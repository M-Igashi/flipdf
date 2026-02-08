use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use glob::glob;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// flipdf - Merge duplex-scanned PDFs into proper page order
///
/// For home scanners (like Brother MFC-J7460DN) that have ADF but no auto-duplex scan.
///
/// Workflow:
/// 1. Scan all front pages with ADF → first PDF
/// 2. Flip the stack, scan all back pages → second PDF (pages in reverse order)
/// 3. Run flipdf to merge them correctly
#[derive(Parser, Debug)]
#[command(name = "flipdf")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Front pages PDF (sequential order). If omitted, auto-detect from current directory
    #[arg(value_name = "FRONTS")]
    fronts: Option<PathBuf>,

    /// Back pages PDF (reverse order from flipped stack)
    #[arg(value_name = "BACKS")]
    backs: Option<PathBuf>,

    /// Output file path [default: merged_YYYYMMDD_HHMMSS.pdf]
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// PDF to prepend at the beginning (e.g., cover page)
    #[arg(long, value_name = "FILE")]
    prepend: Option<PathBuf>,

    /// PDF to append at the end (e.g., appendix)
    #[arg(long, value_name = "FILE")]
    append: Option<PathBuf>,

    /// Don't reverse back pages order (use if backs are already in correct order)
    #[arg(long)]
    no_reverse: bool,

    /// Suppress progress messages
    #[arg(short, long)]
    quiet: bool,

    /// Dry run - show what would be done without executing
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List PDF files in current directory sorted by modification time
    List {
        /// Show all PDFs, not just recent ones
        #[arg(short, long)]
        all: bool,
    },
}

/// Find PDF files in the current directory, sorted by modification time (newest first)
fn find_pdfs_in_current_dir() -> Result<Vec<PathBuf>> {
    let mut pdfs: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

    for pattern in ["*.pdf", "*.PDF"] {
        for path in glob(pattern)
            .context("Failed to read glob pattern")?
            .flatten()
        {
            if pdfs.iter().any(|(p, _)| p == &path) {
                continue;
            }
            if let Ok(metadata) = fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    pdfs.push((path, modified));
                }
            }
        }
    }

    // Sort by modification time, newest first
    pdfs.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(pdfs.into_iter().map(|(p, _)| p).collect())
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

/// Merge multiple PDFs into one using qpdf
fn merge_pdfs(input_paths: &[PathBuf], output_path: &Path) -> Result<()> {
    if input_paths.is_empty() {
        bail!("No input files to merge");
    }

    let page_args: Vec<&str> = input_paths.iter().map(|p| p.to_str().unwrap()).collect();

    let mut args = vec!["--empty", "--pages"];
    args.extend(&page_args);
    args.extend(["--", output_path.to_str().unwrap()]);

    let status = Command::new("qpdf")
        .args(&args)
        .status()
        .context("Failed to run qpdf for merge")?;

    if !status.success() {
        bail!("qpdf failed to merge PDFs");
    }

    Ok(())
}

/// Extract specific pages from a PDF
fn extract_pages(pdf_path: &Path, pages: &str, output_path: &Path) -> Result<()> {
    let input = pdf_path.to_str().unwrap();
    let output = output_path.to_str().unwrap();
    let status = Command::new("qpdf")
        .args([input, "--pages", input, pages, "--", output])
        .status()
        .context("Failed to run qpdf")?;

    if !status.success() {
        bail!("qpdf failed to extract pages from {:?}", pdf_path);
    }

    Ok(())
}

fn generate_output_filename() -> PathBuf {
    let now = chrono::Local::now();
    PathBuf::from(format!("merged_{}.pdf", now.format("%Y%m%d_%H%M%S")))
}

fn list_pdfs(all: bool) -> Result<()> {
    let pdfs = find_pdfs_in_current_dir()?;

    if pdfs.is_empty() {
        println!("No PDF files found in current directory.");
        return Ok(());
    }

    let limit = if all { pdfs.len() } else { pdfs.len().min(10) };

    println!("PDF files in current directory (newest first):\n");
    for (i, pdf) in pdfs.iter().take(limit).enumerate() {
        let metadata = fs::metadata(pdf)?;
        let size_kb = metadata.len() / 1024;
        let pages = get_page_count(pdf).unwrap_or(0);
        println!(
            "  {}. {} ({} pages, {} KB)",
            i + 1,
            pdf.display(),
            pages,
            size_kb
        );
    }

    if !all && pdfs.len() > 10 {
        println!(
            "\n  ... and {} more (use --all to show all)",
            pdfs.len() - 10
        );
    }

    println!("\nTo merge the 2 newest PDFs, just run: flipdf");

    Ok(())
}

fn merge_duplex_scans(cli: &Cli) -> Result<()> {
    let log = |msg: &str| {
        if !cli.quiet {
            println!("{}", msg);
        }
    };

    // Determine input files
    let (fronts_path, backs_path) = match (&cli.fronts, &cli.backs) {
        (Some(f), Some(b)) => (f.clone(), b.clone()),
        (Some(_), None) | (None, Some(_)) => {
            bail!("Please specify both fronts and backs PDFs, or neither for auto-detect");
        }
        (None, None) => {
            // Auto-detect: use the two newest PDFs
            let pdfs = find_pdfs_in_current_dir()?;
            if pdfs.len() < 2 {
                bail!(
                    "Need at least 2 PDF files for auto-detect. Found: {}.\n\
                     Use 'flipdf list' to see available PDFs, or specify files explicitly:\n\
                     flipdf <fronts.pdf> <backs.pdf>",
                    pdfs.len()
                );
            }
            log(&format!(
                "Auto-detected PDFs (newest first):\n  Fronts: {}\n  Backs:  {}",
                pdfs[0].display(),
                pdfs[1].display()
            ));
            // Newest = fronts (scanned first), second newest = backs (scanned after flipping)
            // Actually, backs are scanned AFTER fronts, so backs should be newer
            // Let's use: [0] = backs (newer), [1] = fronts (older)
            (pdfs[1].clone(), pdfs[0].clone())
        }
    };

    // Validate input files exist
    if !fronts_path.exists() {
        bail!("Fronts PDF not found: {:?}", fronts_path);
    }
    if !backs_path.exists() {
        bail!("Backs PDF not found: {:?}", backs_path);
    }

    // Determine output path
    let output_path = cli.output.clone().unwrap_or_else(generate_output_filename);

    if cli.dry_run {
        println!("Dry run - would merge:");
        println!("  Fronts: {}", fronts_path.display());
        println!(
            "  Backs:  {} {}",
            backs_path.display(),
            if cli.no_reverse {
                "(no reverse)"
            } else {
                "(reversed)"
            }
        );
        if let Some(ref p) = cli.prepend {
            println!("  Prepend: {}", p.display());
        }
        if let Some(ref a) = cli.append {
            println!("  Append: {}", a.display());
        }
        println!("  Output: {}", output_path.display());
        return Ok(());
    }

    // Get page counts
    let num_fronts = get_page_count(&fronts_path)?;
    let num_backs = get_page_count(&backs_path)?;

    let reverse_note = if cli.no_reverse {
        ""
    } else {
        " [will be reversed]"
    };
    log(&format!(
        "Fronts: {} ({} pages)",
        fronts_path.display(),
        num_fronts
    ));
    log(&format!(
        "Backs:  {} ({} pages){}",
        backs_path.display(),
        num_backs,
        reverse_note
    ));

    if num_fronts != num_backs {
        log(&format!(
            "⚠ Warning: Page count mismatch - fronts: {}, backs: {}",
            num_fronts, num_backs
        ));
    }

    // Create temp directory for intermediate files
    let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
    let mut temp_pdfs: Vec<PathBuf> = Vec::new();
    let mut total_pages = 0;

    // Back page number: reverse order (flipped stack) unless --no-reverse
    let back_page_num = |i: usize| -> usize {
        if cli.no_reverse {
            i + 1
        } else {
            num_backs - i
        }
    };

    // Handle prepend
    if let Some(ref prepend_path) = cli.prepend {
        if !prepend_path.exists() {
            bail!("Prepend PDF not found: {:?}", prepend_path);
        }
        let prepend_pages = get_page_count(prepend_path)?;
        log(&format!(
            "Prepending: {} ({} pages)",
            prepend_path.display(),
            prepend_pages
        ));
        temp_pdfs.push(prepend_path.clone());
        total_pages += prepend_pages;
    }

    // Interleave pages
    let num_sheets = num_fronts.min(num_backs);
    for i in 0..num_sheets {
        let front_temp = temp_dir.path().join(format!("front_{:04}.pdf", i));
        extract_pages(&fronts_path, &(i + 1).to_string(), &front_temp)?;
        temp_pdfs.push(front_temp);

        let back_temp = temp_dir.path().join(format!("back_{:04}.pdf", i));
        extract_pages(&backs_path, &back_page_num(i).to_string(), &back_temp)?;
        temp_pdfs.push(back_temp);

        total_pages += 2;
    }

    // Handle remaining pages if counts don't match
    if num_fronts > num_backs {
        for i in num_backs..num_fronts {
            let page_num = i + 1;
            let front_temp = temp_dir.path().join(format!("extra_front_{:04}.pdf", i));
            extract_pages(&fronts_path, &page_num.to_string(), &front_temp)?;
            temp_pdfs.push(front_temp);
            log(&format!("Extra front page: {}", page_num));
            total_pages += 1;
        }
    } else if num_backs > num_fronts {
        for i in num_fronts..num_backs {
            let page_num = back_page_num(i);
            let back_temp = temp_dir.path().join(format!("extra_back_{:04}.pdf", i));
            extract_pages(&backs_path, &page_num.to_string(), &back_temp)?;
            temp_pdfs.push(back_temp);
            log(&format!("Extra back page: {}", page_num));
            total_pages += 1;
        }
    }

    // Handle append
    if let Some(ref append_path) = cli.append {
        if !append_path.exists() {
            bail!("Append PDF not found: {:?}", append_path);
        }
        let append_pages = get_page_count(append_path)?;
        log(&format!(
            "Appending: {} ({} pages)",
            append_path.display(),
            append_pages
        ));
        temp_pdfs.push(append_path.clone());
        total_pages += append_pages;
    }

    // Merge all PDFs
    merge_pdfs(&temp_pdfs, &output_path)?;

    log(&format!(
        "\n✓ Created: {} ({} pages)",
        output_path.display(),
        total_pages
    ));

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::List { all }) => list_pdfs(*all),
        None => merge_duplex_scans(&cli),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_detect_mode() {
        let cli = Cli::parse_from(["flipdf"]);
        assert!(cli.fronts.is_none());
        assert!(cli.backs.is_none());
    }

    #[test]
    fn test_explicit_files() {
        let cli = Cli::parse_from(["flipdf", "fronts.pdf", "backs.pdf"]);
        assert_eq!(cli.fronts, Some(PathBuf::from("fronts.pdf")));
        assert_eq!(cli.backs, Some(PathBuf::from("backs.pdf")));
    }

    #[test]
    fn test_with_options() {
        let cli = Cli::parse_from([
            "flipdf",
            "fronts.pdf",
            "backs.pdf",
            "-o",
            "out.pdf",
            "--prepend",
            "cover.pdf",
            "--append",
            "appendix.pdf",
            "--no-reverse",
            "--quiet",
        ]);
        assert_eq!(cli.output, Some(PathBuf::from("out.pdf")));
        assert_eq!(cli.prepend, Some(PathBuf::from("cover.pdf")));
        assert_eq!(cli.append, Some(PathBuf::from("appendix.pdf")));
        assert!(cli.no_reverse);
        assert!(cli.quiet);
    }

    #[test]
    fn test_list_command() {
        let cli = Cli::parse_from(["flipdf", "list"]);
        assert!(matches!(cli.command, Some(Commands::List { all: false })));
    }

    #[test]
    fn test_list_all() {
        let cli = Cli::parse_from(["flipdf", "list", "--all"]);
        assert!(matches!(cli.command, Some(Commands::List { all: true })));
    }
}
