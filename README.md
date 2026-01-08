# flipdf

Merge duplex-scanned PDFs into proper page order — for home scanners without auto-duplex.

## Why?

Many home multifunction printers (like the Brother MFC-7460DN) have ADF (Auto Document Feeder) for continuous scanning, but lack automatic duplex scanning. This means scanning double-sided documents requires:

1. Scan all front pages → `fronts.pdf`
2. Flip the stack, scan all back pages → `backs.pdf` (pages come out in reverse order)
3. Manually interleave them... 😩

**flipdf** automates step 3. Just run `flipdf` in your scan folder and you're done.

## Installation

### Homebrew (macOS)

```bash
brew tap M-Igashi/tap
brew install flipdf
```

### From Source

```bash
git clone https://github.com/M-Igashi/flipdf.git
cd flipdf
cargo install --path .
```


## Usage

### Automatic Mode (Recommended)

Just navigate to your scan folder and run:

```bash
flipdf
```

flipdf will automatically detect the two newest PDFs and merge them. The older PDF is treated as fronts, the newer as backs (since you scan fronts first, then flip and scan backs).

### Explicit Mode

Specify files directly:

```bash
flipdf fronts.pdf backs.pdf
flipdf fronts.pdf backs.pdf -o document.pdf
```

### List PDFs

See what's in the current directory:

```bash
flipdf list
flipdf list --all
```

### Options

| Option | Description |
|--------|-------------|
| `-o, --output <FILE>` | Output filename (default: `merged_YYYYMMDD_HHMMSS.pdf`) |
| `--prepend <FILE>` | Add a PDF at the beginning (e.g., cover page) |
| `--append <FILE>` | Add a PDF at the end (e.g., appendix) |
| `--no-reverse` | Don't reverse back pages (if already in correct order) |
| `-q, --quiet` | Suppress progress messages |
| `--dry-run` | Show what would be done without executing |

### Examples

```bash
# Basic: auto-detect and merge
flipdf

# With cover page
flipdf --prepend cover.pdf

# Explicit files with custom output
flipdf fronts.pdf backs.pdf -o contract.pdf

# Add both cover and appendix
flipdf fronts.pdf backs.pdf --prepend cover.pdf --append appendix.pdf

# Check what would be merged
flipdf --dry-run
```

## How It Works

When you scan a duplex document without auto-duplex:

```
Original document:  [1] [2] [3] [4] [5] [6]
                     ↓   ↓   ↓   ↓   ↓   ↓
                    F1  B1  F2  B2  F3  B3  (F=front, B=back)

Scan fronts (ADF):  F1, F2, F3 → fronts.pdf (pages 1,2,3)

Flip stack & scan:  B3, B2, B1 → backs.pdf (pages 1,2,3 = original B3,B2,B1)

flipdf merges:      F1, B1, F2, B2, F3, B3 → merged.pdf (correct order!)
```

## License

MIT
