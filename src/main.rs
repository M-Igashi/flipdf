use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::cmp::Reverse;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_OUTPUT_PREFIX: &str = "merged_";

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

/// Find PDF files in the current directory, sorted by modification time (newest first).
fn find_pdfs_in_current_dir() -> Result<Vec<(PathBuf, fs::Metadata)>> {
    let mut pdfs = Vec::new();

    for entry in fs::read_dir(".")
        .context("Failed to read current directory")?
        .flatten()
    {
        let path = PathBuf::from(entry.file_name());
        let is_pdf = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
        if !is_pdf {
            continue;
        }
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.is_file() {
                pdfs.push((path, metadata));
            }
        }
    }

    pdfs.sort_by_key(|(_, metadata)| Reverse(metadata.modified().ok()));

    Ok(pdfs)
}

/// Run qpdf and return its stdout. Exit code 3 means success with warnings.
fn run_qpdf<I, S>(args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("qpdf")
        .args(args)
        .output()
        .context("Failed to run qpdf. Is qpdf installed? (brew install qpdf)")?;

    if !matches!(output.status.code(), Some(0 | 3)) {
        bail!(
            "qpdf failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(output.stdout)
}

fn get_page_count(pdf_path: &Path) -> Result<usize> {
    let stdout = run_qpdf([OsStr::new("--show-npages"), pdf_path.as_os_str()])
        .with_context(|| format!("Failed to get page count of {}", pdf_path.display()))?;

    String::from_utf8_lossy(&stdout)
        .trim()
        .parse()
        .context("Failed to parse page count")
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Side {
    Front,
    Back,
}

/// Interleave fronts and backs as 1-based page numbers.
/// Leftover pages of the longer side follow at the end.
fn page_order(num_fronts: usize, num_backs: usize, reverse_backs: bool) -> Vec<(Side, usize)> {
    let mut order = Vec::with_capacity(num_fronts + num_backs);

    for i in 0..num_fronts.max(num_backs) {
        if i < num_fronts {
            order.push((Side::Front, i + 1));
        }
        if i < num_backs {
            let page = if reverse_backs { num_backs - i } else { i + 1 };
            order.push((Side::Back, page));
        }
    }

    order
}

fn is_same_file(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

fn generate_output_filename() -> PathBuf {
    let now = chrono::Local::now();
    PathBuf::from(format!(
        "{}{}.pdf",
        DEFAULT_OUTPUT_PREFIX,
        now.format("%Y%m%d_%H%M%S")
    ))
}

fn list_pdfs(all: bool) -> Result<()> {
    let pdfs = find_pdfs_in_current_dir()?;

    if pdfs.is_empty() {
        println!("No PDF files found in current directory.");
        return Ok(());
    }

    let limit = if all { pdfs.len() } else { pdfs.len().min(10) };

    println!("PDF files in current directory (newest first):\n");
    for (i, (path, metadata)) in pdfs.iter().take(limit).enumerate() {
        let size_kb = metadata.len() / 1024;
        let pages = get_page_count(path).map_or("?".to_string(), |n| n.to_string());
        println!(
            "  {}. {} ({} pages, {} KB)",
            i + 1,
            path.display(),
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

    let (fronts_path, backs_path) = match (&cli.fronts, &cli.backs) {
        (Some(f), Some(b)) => (f.clone(), b.clone()),
        (Some(_), None) | (None, Some(_)) => {
            bail!("Please specify both fronts and backs PDFs, or neither for auto-detect");
        }
        (None, None) => {
            // Skip outputs of earlier runs so they are not picked up as scans
            let pdfs: Vec<PathBuf> = find_pdfs_in_current_dir()?
                .into_iter()
                .map(|(path, _)| path)
                .filter(|path| !path.to_string_lossy().starts_with(DEFAULT_OUTPUT_PREFIX))
                .collect();
            // Backs are scanned after fronts, so the newest is backs
            let [backs, fronts, ..] = pdfs.as_slice() else {
                bail!(
                    "Need at least 2 PDF files for auto-detect. Found: {}.\n\
                     Use 'flipdf list' to see available PDFs, or specify files explicitly:\n\
                     flipdf <fronts.pdf> <backs.pdf>",
                    pdfs.len()
                );
            };
            log(&format!(
                "Auto-detected PDFs (newest first):\n  Fronts: {}\n  Backs:  {}",
                fronts.display(),
                backs.display()
            ));
            (fronts.clone(), backs.clone())
        }
    };

    let output_path = cli.output.clone().unwrap_or_else(generate_output_filename);

    let inputs = [
        ("Fronts", Some(&fronts_path)),
        ("Backs", Some(&backs_path)),
        ("Prepend", cli.prepend.as_ref()),
        ("Append", cli.append.as_ref()),
    ];
    for (label, path) in inputs {
        let Some(path) = path else { continue };
        if !path.is_file() {
            bail!("{} PDF not found: {}", label, path.display());
        }
        if is_same_file(path, &output_path) {
            bail!(
                "Output would overwrite the {} PDF: {}",
                label.to_lowercase(),
                path.display()
            );
        }
    }

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

    let num_fronts = get_page_count(&fronts_path)?;
    let num_backs = get_page_count(&backs_path)?;
    let mut total_pages = num_fronts + num_backs;

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
        eprintln!(
            "⚠ Warning: Page count mismatch - fronts: {}, backs: {}. Unpaired pages go at the end.",
            num_fronts, num_backs
        );
    }

    // A single qpdf call: --pages takes (file, range) pairs in output order
    let mut args: Vec<OsString> = vec!["--empty".into(), "--pages".into()];

    if let Some(ref prepend_path) = cli.prepend {
        let pages = get_page_count(prepend_path)?;
        log(&format!(
            "Prepending: {} ({} pages)",
            prepend_path.display(),
            pages
        ));
        args.extend([prepend_path.into(), "1-z".into()]);
        total_pages += pages;
    }

    for (side, page) in page_order(num_fronts, num_backs, !cli.no_reverse) {
        let path = match side {
            Side::Front => &fronts_path,
            Side::Back => &backs_path,
        };
        args.extend([path.into(), page.to_string().into()]);
    }

    if let Some(ref append_path) = cli.append {
        let pages = get_page_count(append_path)?;
        log(&format!(
            "Appending: {} ({} pages)",
            append_path.display(),
            pages
        ));
        args.extend([append_path.into(), "1-z".into()]);
        total_pages += pages;
    }

    args.extend(["--".into(), output_path.as_os_str().to_owned()]);
    run_qpdf(&args).context("Failed to merge PDFs")?;

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

    #[test]
    fn test_page_order_reversed_backs() {
        use Side::{Back, Front};
        assert_eq!(
            page_order(3, 3, true),
            [
                (Front, 1),
                (Back, 3),
                (Front, 2),
                (Back, 2),
                (Front, 3),
                (Back, 1)
            ]
        );
    }

    #[test]
    fn test_page_order_no_reverse() {
        use Side::{Back, Front};
        assert_eq!(
            page_order(2, 2, false),
            [(Front, 1), (Back, 1), (Front, 2), (Back, 2)]
        );
    }

    #[test]
    fn test_page_order_mismatch() {
        use Side::{Back, Front};
        assert_eq!(
            page_order(3, 1, true),
            [(Front, 1), (Back, 1), (Front, 2), (Front, 3)]
        );
        assert_eq!(
            page_order(1, 3, true),
            [(Front, 1), (Back, 3), (Back, 2), (Back, 1)]
        );
    }
}
