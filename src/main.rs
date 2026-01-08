use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Merge duplex-scanned PDFs into proper page order.
///
/// When scanning duplex documents without an ADF:
/// 1. Scan all front pages sequentially → fronts.pdf (pages 1,2,3...)
/// 2. Flip the stack and scan all back pages → backs.pdf (pages n,n-1,n-2... in reverse)
///
/// This tool interleaves them into proper order: front1, back1, front2, back2...
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// PDF with front pages (sequential order)
    fronts: PathBuf,

    /// PDF with back pages (reverse order from flipped stack)
    backs: PathBuf,

    /// Output merged PDF path
    output: PathBuf,

    /// PDF to add at the beginning
    #[arg(long, value_name = "FILE")]
    prepend: Option<PathBuf>,

    /// PDF to add at the end
    #[arg(long, value_name = "FILE")]
    append: Option<PathBuf>,

    /// Suppress progress messages
    #[arg(short, long)]
    quiet: bool,
}

/// Get the number of pages in a PDF using qpdf
fn get_page_count(pdf_path: &Path) -> Result<usize> {
    let output = Command::new("qpdf")
        .args(["--show-npages", pdf_path.to_str().unwrap()])
        .output()
        .context("Failed to run qpdf. Is qpdf installed? (brew install qpdf)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("qpdf failed to get page count: {}", stderr);
    }

    let count_str = String::from_utf8_lossy(&output.stdout);
    count_str
        .trim()
        .parse()
        .context("Failed to parse page count")
}

/// Extract a single page from a PDF to a temporary file
fn extract_page(pdf_path: &Path, page_num: usize, output_path: &Path) -> Result<()> {
    let status = Command::new("qpdf")
        .args([
            pdf_path.to_str().unwrap(),
            "--pages",
            pdf_path.to_str().unwrap(),
            &page_num.to_string(),
            "--",
            output_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to run qpdf")?;

    if !status.success() {
        bail!("qpdf failed to extract page {} from {:?}", page_num, pdf_path);
    }

    Ok(())
}

/// Merge multiple PDFs into one using qpdf
fn merge_pdfs(input_paths: &[PathBuf], output_path: &Path) -> Result<()> {
    if input_paths.is_empty() {
        bail!("No input files to merge");
    }

    let mut args: Vec<String> = vec!["--empty".to_string()];
    args.push("--pages".to_string());
    
    for path in input_paths {
        args.push(path.to_str().unwrap().to_string());
    }
    
    args.push("--".to_string());
    args.push(output_path.to_str().unwrap().to_string());

    let status = Command::new("qpdf")
        .args(&args)
        .status()
        .context("Failed to run qpdf for merge")?;

    if !status.success() {
        bail!("qpdf failed to merge PDFs");
    }

    Ok(())
}

fn merge_duplex_scans(args: &Args) -> Result<usize> {
    let log = |msg: &str| {
        if !args.quiet {
            println!("{}", msg);
        }
    };

    // Validate input files exist
    if !args.fronts.exists() {
        bail!("Fronts PDF not found: {:?}", args.fronts);
    }
    if !args.backs.exists() {
        bail!("Backs PDF not found: {:?}", args.backs);
    }

    // Get page counts
    let num_fronts = get_page_count(&args.fronts)?;
    let num_backs = get_page_count(&args.backs)?;

    if num_fronts != num_backs {
        log(&format!(
            "Warning: Page count mismatch - fronts: {}, backs: {}",
            num_fronts, num_backs
        ));
    }

    // Create temp directory for intermediate files
    let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
    let mut temp_pdfs: Vec<PathBuf> = Vec::new();
    let mut total_pages = 0;

    // Handle prepend
    if let Some(ref prepend_path) = args.prepend {
        if !prepend_path.exists() {
            bail!("Prepend PDF not found: {:?}", prepend_path);
        }
        let prepend_pages = get_page_count(prepend_path)?;
        log(&format!("Prepended: {:?} ({} pages)", prepend_path, prepend_pages));
        temp_pdfs.push(prepend_path.clone());
        total_pages += prepend_pages;
    }

    // Interleave pages
    let num_sheets = num_fronts.min(num_backs);
    for i in 0..num_sheets {
        // Front page: sequential order (1-indexed)
        let front_page_num = i + 1;
        let front_temp = temp_dir.path().join(format!("front_{:04}.pdf", i));
        extract_page(&args.fronts, front_page_num, &front_temp)?;
        temp_pdfs.push(front_temp);

        // Back page: reverse order (flipped stack)
        let back_index = num_backs - 1 - i;
        let back_page_num = back_index + 1;
        let back_temp = temp_dir.path().join(format!("back_{:04}.pdf", i));
        extract_page(&args.backs, back_page_num, &back_temp)?;
        temp_pdfs.push(back_temp);

        log(&format!(
            "Sheet {}: front page {} + back page {}",
            i + 1,
            front_page_num,
            back_page_num
        ));
        total_pages += 2;
    }

    // Handle remaining pages if counts don't match
    if num_fronts > num_backs {
        for i in num_backs..num_fronts {
            let front_page_num = i + 1;
            let front_temp = temp_dir.path().join(format!("extra_front_{:04}.pdf", i));
            extract_page(&args.fronts, front_page_num, &front_temp)?;
            temp_pdfs.push(front_temp);
            log(&format!("Extra front page: {}", front_page_num));
            total_pages += 1;
        }
    } else if num_backs > num_fronts {
        for i in num_fronts..num_backs {
            let back_index = num_backs - 1 - i;
            let back_page_num = back_index + 1;
            let back_temp = temp_dir.path().join(format!("extra_back_{:04}.pdf", i));
            extract_page(&args.backs, back_page_num, &back_temp)?;
            temp_pdfs.push(back_temp);
            log(&format!("Extra back page: {}", back_page_num));
            total_pages += 1;
        }
    }

    // Handle append
    if let Some(ref append_path) = args.append {
        if !append_path.exists() {
            bail!("Append PDF not found: {:?}", append_path);
        }
        let append_pages = get_page_count(append_path)?;
        log(&format!("Appended: {:?} ({} pages)", append_path, append_pages));
        temp_pdfs.push(append_path.clone());
        total_pages += append_pages;
    }

    // Merge all PDFs
    merge_pdfs(&temp_pdfs, &args.output)?;

    log(&format!("\nOutput: {:?} ({} pages)", args.output, total_pages));

    Ok(total_pages)
}

fn main() -> Result<()> {
    let args = Args::parse();
    merge_duplex_scans(&args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_parsing() {
        let args = Args::parse_from(&[
            "duplex-scan-merger",
            "fronts.pdf",
            "backs.pdf",
            "output.pdf",
        ]);
        assert_eq!(args.fronts, PathBuf::from("fronts.pdf"));
        assert_eq!(args.backs, PathBuf::from("backs.pdf"));
        assert_eq!(args.output, PathBuf::from("output.pdf"));
        assert!(args.prepend.is_none());
        assert!(args.append.is_none());
    }

    #[test]
    fn test_args_with_options() {
        let args = Args::parse_from(&[
            "duplex-scan-merger",
            "fronts.pdf",
            "backs.pdf",
            "output.pdf",
            "--prepend",
            "cover.pdf",
            "--append",
            "appendix.pdf",
            "--quiet",
        ]);
        assert_eq!(args.prepend, Some(PathBuf::from("cover.pdf")));
        assert_eq!(args.append, Some(PathBuf::from("appendix.pdf")));
        assert!(args.quiet);
    }
}
