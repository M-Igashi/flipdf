# duplex-scan-merger

A fast, native macOS CLI tool to merge duplex-scanned PDFs into proper page order.

## The Problem

When scanning duplex (double-sided) documents without an ADF (Automatic Document Feeder):

1. First, you scan all **front pages** sequentially → `fronts.pdf` (pages 1, 2, 3...)
2. Then you flip the stack and scan all **back pages** → `backs.pdf` (pages n, n-1, n-2... in reverse order)

This tool interleaves them into the correct reading order: front1, back1, front2, back2...

## Installation

### Using Homebrew

```bash
brew tap higashi-masanari/tap
brew install duplex-scan-merger
```

### From Source

```bash
cargo install --git https://github.com/higashi-masanari/duplex-scan-merger
```

## Usage

```bash
duplex-scan-merger <fronts.pdf> <backs.pdf> <output.pdf> [OPTIONS]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `fronts.pdf` | PDF with front pages (sequential order) |
| `backs.pdf` | PDF with back pages (reverse order from flipped stack) |
| `output.pdf` | Output merged PDF path |

### Options

| Option | Description |
|--------|-------------|
| `--prepend <FILE>` | PDF to add at the beginning (e.g., cover page) |
| `--append <FILE>` | PDF to add at the end (e.g., appendix) |
| `-q, --quiet` | Suppress progress messages |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

### Examples

Basic merge:
```bash
duplex-scan-merger fronts.pdf backs.pdf merged.pdf
```

With a cover page:
```bash
duplex-scan-merger fronts.pdf backs.pdf output.pdf --prepend cover.pdf
```

With both prepend and append:
```bash
duplex-scan-merger fronts.pdf backs.pdf output.pdf --prepend cover.pdf --append appendix.pdf
```

## How It Works

The tool handles the common duplex scanning workflow:

```
Original document:    Scanned fronts:    Scanned backs (flipped):    Result:
┌───┐ ┌───┐ ┌───┐    [1, 2, 3]          [6, 5, 4]                    [1,4,2,5,3,6]
│ 1 │ │ 2 │ │ 3 │         ↓                  ↓
├───┤ ├───┤ ├───┤    fronts.pdf          backs.pdf
│ 4 │ │ 5 │ │ 6 │
└───┘ └───┘ └───┘
```

The backs are reversed (since the stack was flipped), and pages are interleaved to restore the correct order.

## Building from Source

```bash
git clone https://github.com/higashi-masanari/duplex-scan-merger
cd duplex-scan-merger
cargo build --release
```

The binary will be at `target/release/duplex-scan-merger`.

## License

MIT License - see [LICENSE](LICENSE) for details.
