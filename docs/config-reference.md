# BIBR Configuration

BIBR looks for configuration at `~/.config/bibr/bibr.toml`.

## Example Configuration

```toml
# BibTeX files to load (required)
bibtex_files = [
    "~/3_resources/research/refs/zotero.bib",
    "~/papers/references.bib",
]

# Search settings
[search]
smart_case = true
fuzzy = true
search_all_fields = true

# Display format for entry list
[display]
format = "{author} - {title} ({year})"

# Keybindings (vim-style defaults)
[keybindings]
up = "k"
down = "j"
search = "/"
quit = "q"
edit = "e"
note = "n"
pdf = "p"
copy = "y"
sort_year = "Y"
sort_author = "A"
preview = "i"
"preview[0]" = "1"
"preview[1,2]" = "2"

# Theme colors (16-color terminal)
[theme]
selected_fg = "black"
selected_bg = "green"
highlight_fg = "yellow"
list_border_fg = "gray"
list_border_bg = "black"

# External tools
editor = "nvim"  # Uses $EDITOR if not set
pdf_reader = "open"  # macOS 'open', Linux 'xdg-open', etc.

# Notes configuration
[notes]
dir = "~/.local/share/bibr/notes"
filename_pattern = "{citekey}.md"
template_file = "~/.config/bibr/template.md"
```

> Note: TOML requires quoting keys that contain brackets or commas.
> Use `"preview[0]"` and `"preview[1,2]"` (quoted), not `preview[0]`.

## Filename Pattern Variables

Use these variables in `notes.filename_pattern`:

### Built-in Variables

- `{citekey}` - Entry citation key
- `{authors}` - List of authors in their original BibTeX format
- `{title}` - Entry title (truncated in filenames)
- `{year}` - Publication year
- `{date}` - Current date (default: %Y-%m-%d)
- `{time}` - Current time (default: %H:%M:%S)
- `{datetime}` - Current datetime (default: %Y-%m-%d %H:%M:%S)
- `{date:FORMAT}`, `{time:FORMAT}`, `{datetime:FORMAT}` - Custom format using [Chrono strftime specifiers](https://docs.rs/chrono/latest/chrono/format/strftime/index.html)

### Arbitrary BibTeX Fields

You can use any field from the BibTeX entry:
- `{journal}` - Journal name
- `{volume}` - Volume number
- `{pages}` - Page numbers
- `{publisher}` - Publisher
- `{file}` - File path
- Any other field present in the entry

If a field doesn't exist in the entry:
- Without default: the placeholder is kept as-is (e.g., `{publisher}`)
- With default: the default value is used (see below)

### Default Values

Use `/default` syntax to provide a fallback when a field is missing:

```toml
filename_pattern = "{citekey}-{journal/unknown}.md"
```

If `journal` field exists → uses the journal value  
If `journal` field missing → uses "unknown"

This works for any variable:
- `{volume/N/A}` → "N/A" if volume not present
- `{publisher/self-published}` → "self-published" if publisher not present

### Authors List Operations

The `{authors}` variable is a **list** where each author retains their original BibTeX format:
- If BibTeX has `"Doe, Jane and Smith, John"` → authors are `["Doe, Jane", "Smith, John"]`
- If BibTeX has `"Jane Doe and John Smith"` → authors are `["Jane Doe", "John Smith"]`

**Format strings** (to reformat author names):
- `{authors:%F %L}` - "First Last" format (e.g., "Jane Doe, John Smith")
- `{authors:%L, %F}` - "Last, First" format (e.g., "Doe, Jane, Smith, John")
- `{authors:%L}` - Last names only (e.g., "Doe, Smith")
- `{authors}` - Original format unchanged

**Slice transform:**
- `{authors:slice(0,3)}` - First 3 authors only
- `{authors:slice(1,3)}` - Authors 2-3

**Join transforms:**
- `{authors:join( and )}` - Join with " and " (e.g., "Jane Doe and John Smith")
- `{authors:join(, )}` - Join with comma and space
- `{authors:join}` - Default join (uses surnames for filenames)

**Case transforms:**
- `{authors:lower}` - All authors lowercase
- `{authors:upper}` - All authors uppercase

**Chaining transforms:**
Transforms are applied left to right: `format → slice → join → case`

Example: `{authors:%F %L:slice(0,3):join( and ):lower}`
1. Format as "First Last"
2. Take first 3 authors
3. Join with " and "
4. Convert to lowercase

### String Transforms (General)

- `{citekey:lower}` - lowercase
- `{citekey:upper}` - uppercase
- `{title:slice(0,50)}` - first 50 chars

Transforms work with any field:
- `{journal:lower}` - lowercase journal name
- `{publisher:slice(0,20)}` - first 20 chars of publisher

### Date/Time Format Specifiers

Use standard Chrono strftime format strings:

- `%Y` - Full year (e.g., 2024)
- `%y` - Short year (e.g., 24)
- `%m` - Month (01-12)
- `%d` - Day (01-31)
- `%H` - Hour 00-23
- `%M` - Minute (00-59)
- `%S` - Second (00-59)

Examples:
- `{date:%Y-%m}` → "2024-03"
- `{date:%Y%m%d}` → "20240321"
- `{time:%H:%M}` → "14:30"

## Note Template

Create `~/.config/bibr/template.md`:

```markdown
---
citekey: {citekey}
authors: {authors}
authors_formatted: {authors:%F %L}
title: {title}
year: {year}
journal: {journal/unknown}
volume: {volume/N/A}
pages: {pages}
---

# Notes on {title}

Authors: {authors:%F %L:join( and )}

Published in: {journal/unknown}

## Summary

## Key Points

## Personal Thoughts

## References
```

**Template variables:**
- All built-in variables: `{citekey}`, `{authors}`, `{title}`, `{year}`, `{date}`, `{time}`, etc.
- Any BibTeX field: `{journal}`, `{volume}`, `{pages}`, `{publisher}`, `{file}`, etc.
- Default values: `{field/default}` uses "default" if field is missing
- Transforms: `{journal:lower}`, `{title:slice(0,50)}`

**Authors variable:**
- `{authors}` - Original BibTeX format (unchanged)
- `{authors:%F %L}` - Reformat to "First Last"
- `{authors:%L, %F}` - Reformat to "Last, First"
- `{authors:slice(0,2)}` - First 2 authors
- `{authors:join( and )}` - Join with separator
- `{authors:%F %L:slice(0,3):join(, )}` - Chain: format → slice → join

If a field doesn't exist and no default is provided, the placeholder is kept as-is (e.g., `{publisher}`).

## Search Syntax

- Plain text: `machine learning` - searches all fields
- Field qualified: `@title: machine learning` - searches title only
- Multiple: `@author: smith @year: 2023` - combined with AND

Smart case:
- `machine` - matches Machine, MACHINE, etc.
- `MACHINE` - matches only MACHINE (case-sensitive)
