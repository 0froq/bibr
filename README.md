# BIBR

BibTeX Reference Manager - A TUI and CLI application for managing BibTeX files.

## Features

- **TUI Interface**: Interactive terminal UI with vim-style keybindings
- **CLI Commands**: Scriptable command-line interface
- **Multi-file Support**: Load and manage multiple .bib files
- **Real-time Search**: Fuzzy search with field-qualified queries (@title:, @author:)
- **Smart Case**: Case-insensitive by default, case-sensitive when uppercase present
- **Notes**: Create markdown notes linked to entries with templates
- **PDF Integration**: Open associated PDFs
- **Editor Integration**: Edit entries in your preferred editor
- **Clipboard**: Copy citekeys to clipboard
- **Configurable**: Extensive TOML configuration

## Installation

### From Source (Cargo)

```bash
cargo install --path .
```

### Requirements

- Rust 1.70+
- For clipboard support: platform-appropriate dependencies (see arboard crate)

## Quick Start

1. **Initialize config**:
   ```bash
   bibr init
   ```

2. **Edit config** at `~/.config/bibr/bibr.toml` (automatically created):
   ```toml
   bibtex_files = ["~/path/to/your.bib"]
   ```

3. **Launch TUI**:
   ```bash
   bibr
   ```

## Usage

### TUI Mode

Default mode when running `bibr` without subcommands.

**Navigation**:
- `j` / `k` - down/up
- `Ctrl+d` / `Ctrl+u` - page down/up
- `gg` / `G` - top/bottom
- `/` - search
- `q` - quit

**Actions**:
- `Enter` - select entry
- `e` - edit entry
- `n` - create/open note
- `p` - open PDF
- `y` - copy citekey
- `Y` - copy and quit
- `s` - sort menu

**Search**:
- Type to search in real-time
- `@title: query` - search title only
- `@author: name` - search author only

### CLI Commands

```bash
# List all entries
bibr list

# Search and display
bibr search "machine learning"

# Show entry details
bibr show knuth1984

# Edit entry
bibr edit knuth1984

# Create note
bibr note knuth1984

# Copy citekey
bibr copy knuth1984

# Open PDF
bibr pdf knuth1984
```

## Configuration

See [docs/config-reference.md](docs/config-reference.md) for full configuration options.

## Testing

```bash
cargo test
```

## License

MIT
