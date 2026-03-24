use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};

use crate::domain::{Bibliography, Entry};

pub struct BibStore;

impl BibStore {
    pub fn save_entry(entry: &Entry, bib: &Bibliography) -> Result<()> {
        let stored = bib
            .get(&entry.id)
            .ok_or_else(|| anyhow!("entry `{}` is not present in bibliography", entry.id))?;

        if stored.provenance.file_path != entry.provenance.file_path {
            bail!(
                "entry `{}` provenance does not match bibliography source `{}`",
                entry.id,
                stored.provenance.file_path.display()
            );
        }

        Self::update_entry_in_file(entry)
    }

    pub fn update_entry_in_file(entry: &Entry) -> Result<()> {
        let path = &entry.provenance.file_path;
        let original = fs::read_to_string(path)
            .with_context(|| format!("failed to read bibliography file `{}`", path.display()))?;

        let block = locate_entry_block(&original, entry)
            .ok_or_else(|| anyhow!("unable to locate entry `{}` in `{}`", entry.id, path.display()))?;

        let replacement = render_entry(entry, block.text);
        let mut updated = String::with_capacity(original.len() + replacement.len());
        updated.push_str(&original[..block.start]);
        updated.push_str(&replacement);
        updated.push_str(&original[block.end..]);

        let latest = fs::read_to_string(path).with_context(|| {
            format!(
                "failed to re-read bibliography file `{}` before writing",
                path.display()
            )
        })?;

        if latest != original {
            bail!(
                "bibliography file `{}` changed while updating `{}`",
                path.display(),
                entry.id
            );
        }

        let _backup_path = create_backup(path, &original)?;
        atomic_write(path, &updated)?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct EntryBlock<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn locate_entry_block<'a>(source: &'a str, entry: &Entry) -> Option<EntryBlock<'a>> {
    let byte_start = entry.provenance.byte_start.min(source.len());
    let byte_end = entry.provenance.byte_end.min(source.len());

    if byte_start < byte_end {
        let candidate = &source[byte_start..byte_end];
        if starts_like_entry_block(candidate)
            && extract_entry_key(candidate).as_deref() == Some(entry.id.0.as_str())
        {
            return Some(EntryBlock { start: byte_start, end: byte_end, text: candidate });
        }
    }

    scan_entry_blocks(source)
        .into_iter()
        .find(|block| extract_entry_key(block.text).as_deref() == Some(entry.id.0.as_str()))
}

fn render_entry(entry: &Entry, original_block: &str) -> String {
    let newline = if original_block.contains("\r\n") { "\r\n" } else { "\n" };
    let style = render_style(original_block);
    let ordered_fields = ordered_fields(entry, original_block);
    let mut rendered = String::new();

    rendered.push('@');
    rendered.push_str(&style.entry_type);
    rendered.push_str(&style.space_before_delimiter);
    rendered.push(style.open_delimiter);
    rendered.push_str(&entry.id.0);
    rendered.push(',');
    rendered.push_str(newline);

    for (index, (key, value)) in ordered_fields.iter().enumerate() {
        rendered.push_str(&style.field_indent);
        rendered.push_str(key);
        rendered.push_str(" = {");
        rendered.push_str(value);
        rendered.push('}');
        if index + 1 < ordered_fields.len() {
            rendered.push(',');
        }
        rendered.push_str(newline);
    }

    rendered.push_str(&style.closing_indent);
    rendered.push(style.close_delimiter);

    if original_block.ends_with("\r\n") {
        rendered.push_str("\r\n");
    } else if original_block.ends_with('\n') {
        rendered.push('\n');
    }

    rendered
}

#[derive(Debug)]
struct RenderStyle {
    entry_type: String,
    space_before_delimiter: String,
    field_indent: String,
    closing_indent: String,
    open_delimiter: char,
    close_delimiter: char,
}

fn render_style(original_block: &str) -> RenderStyle {
    let mut chars = original_block.char_indices();
    let mut entry_type = String::new();
    let mut space_before_delimiter = String::new();
    let mut open_delimiter = '{';

    while let Some((_, ch)) = chars.next() {
        if ch == '@' {
            break;
        }
    }

    for (_, ch) in chars.by_ref() {
        if ch.is_ascii_alphabetic() {
            entry_type.push(ch);
        } else if ch.is_ascii_whitespace() {
            space_before_delimiter.push(ch);
        } else if matches!(ch, '{' | '(') {
            open_delimiter = ch;
            break;
        } else {
            break;
        }
    }

    let close_delimiter = if open_delimiter == '(' { ')' } else { '}' };
    let field_indent = original_block
        .lines()
        .skip(1)
        .find_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with(close_delimiter) {
                None
            } else {
                Some(line[..line.len() - trimmed.len()].to_string())
            }
        })
        .unwrap_or_else(|| "  ".to_string());
    let closing_indent = original_block
        .lines()
        .last()
        .and_then(|line| line.find(close_delimiter).map(|index| line[..index].to_string()))
        .unwrap_or_default();

    RenderStyle {
        entry_type: if entry_type.is_empty() {
            "article".to_string()
        } else {
            entry_type
        },
        space_before_delimiter,
        field_indent,
        closing_indent,
        open_delimiter,
        close_delimiter,
    }
}

fn ordered_fields(entry: &Entry, original_block: &str) -> Vec<(String, String)> {
    let mut fields = entry
        .fields
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    fields.sort_by(|left, right| left.0.cmp(&right.0));

    let mut ordered = Vec::with_capacity(fields.len());
    for key in field_keys_in_block(original_block) {
        if let Some(index) = fields.iter().position(|(candidate, _)| candidate == &key) {
            ordered.push(fields.remove(index));
        }
    }
    ordered.extend(fields);
    ordered
}

fn field_keys_in_block(block: &str) -> Vec<String> {
    let mut keys = Vec::new();

    for line in block.lines().skip(1) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('}') || trimmed.starts_with(')') {
            continue;
        }

        let mut end = 0usize;
        for ch in trimmed.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                end += ch.len_utf8();
            } else {
                break;
            }
        }

        if end == 0 {
            continue;
        }

        let key = trimmed[..end].to_ascii_lowercase();
        let rest = trimmed[end..].trim_start();
        if rest.starts_with('=') && !keys.contains(&key) {
            keys.push(key);
        }
    }

    keys
}

fn starts_like_entry_block(block: &str) -> bool {
    matches!(block.chars().find(|ch| !ch.is_whitespace()), Some('@'))
}

fn create_backup(path: &Path, contents: &str) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid bibliography path `{}`", path.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_nanos();
    let backup_path = path.with_file_name(format!("{file_name}.{timestamp}.bak"));

    fs::write(&backup_path, contents).with_context(|| {
        format!(
            "failed to create backup `{}` for bibliography file `{}`",
            backup_path.display(),
            path.display()
        )
    })?;

    Ok(backup_path)
}

fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid bibliography path `{}`", path.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_nanos();
    let temp_path = path.with_file_name(format!("{file_name}.{timestamp}.tmp"));

    fs::write(&temp_path, contents).with_context(|| {
        format!(
            "failed to write temporary bibliography file `{}`",
            temp_path.display()
        )
    })?;

    if let Ok(metadata) = fs::metadata(path) {
        let _ = fs::set_permissions(&temp_path, metadata.permissions());
    }

    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to replace bibliography file `{}` with updated content",
            path.display()
        )
    })?;

    Ok(())
}

fn scan_entry_blocks(source: &str) -> Vec<EntryBlock<'_>> {
    let bytes = source.as_bytes();
    let mut index = 0usize;
    let mut blocks = Vec::new();

    while index < bytes.len() {
        if bytes[index] != b'@' {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        if index >= bytes.len() || !matches!(bytes[index], b'{' | b'(') {
            continue;
        }

        let mut brace_depth = if bytes[index] == b'{' { 1usize } else { 0usize };
        let mut paren_depth = if bytes[index] == b'(' { 1usize } else { 0usize };
        let mut in_quotes = false;
        let mut escaped = false;
        index += 1;

        while index < bytes.len() {
            let byte = bytes[index];

            if in_quotes {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_quotes = false;
                }
                index += 1;
                continue;
            }

            match byte {
                b'"' => in_quotes = true,
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                _ => {}
            }

            index += 1;

            if brace_depth == 0 && paren_depth == 0 {
                break;
            }
        }

        let end = index.min(bytes.len());
        let block = &source[start..end];
        if extract_entry_key(block).is_some() {
            blocks.push(EntryBlock { start, end, text: block });
        }
    }

    blocks
}

fn extract_entry_key(block: &str) -> Option<String> {
    let at = block.find('@')?;
    let mut index = at + 1;
    let bytes = block.as_bytes();

    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
        index += 1;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    if index >= bytes.len() || !matches!(bytes[index], b'{' | b'(') {
        return None;
    }
    index += 1;

    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    let key_start = index;
    while index < bytes.len() {
        match bytes[index] {
            b',' | b'\n' | b'\r' | b'}' | b')' => break,
            _ => index += 1,
        }
    }

    let key = block[key_start..index].trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use crate::domain::{load_from_file, EntryId};

    use super::*;

    #[test]
    fn save_entry_updates_only_target_block_and_creates_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("library.bib");
        let source = concat!(
            "% header comment\n",
            "\n",
            "@article{alpha,\n",
            "    title = {Alpha Title},\n",
            "    author = {Alpha, Ada},\n",
            "    year = {2020}\n",
            "}\n",
            "\n",
            "% separator\n",
            "@book{beta,\n",
            "    title = {Beta Title},\n",
            "    author = {Beta, Bob},\n",
            "    year = {2021}\n",
            "}\n"
        );
        fs::write(&path, source).unwrap();

        let mut bibliography = load_from_file(&path).unwrap();
        let entry = bibliography.get_mut(&EntryId::from("alpha")).unwrap();
        entry.set_field("title", "Updated Title");
        let updated_entry = entry.clone();

        BibStore::save_entry(&updated_entry, &bibliography).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("title = {Updated Title}"));
        assert!(written.contains("% header comment"));
        assert!(written.contains("% separator"));
        assert!(written.contains("@book{beta,"));
        assert!(written.contains("    title = {Beta Title}"));

        let backups = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.starts_with("library.bib.") && name.ends_with(".bak"))
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(&backups[0]).unwrap(), source);
    }

    #[test]
    fn update_entry_relocates_using_entry_id_when_provenance_is_stale() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("library.bib");
        let original = concat!(
            "@article{alpha,\n",
            "  title = {Alpha},\n",
            "  year = {2020}\n",
            "}\n"
        );
        fs::write(&path, original).unwrap();

        let mut bibliography = load_from_file(&path).unwrap();
        let entry = bibliography.get_mut(&EntryId::from("alpha")).unwrap();
        entry.set_field("title", "Relocated");
        let mut detached = entry.clone();

        let prefixed = format!("% inserted later\n\n{original}");
        fs::write(&path, prefixed).unwrap();
        detached.provenance.byte_start = 0;
        detached.provenance.byte_end = original.len();

        BibStore::update_entry_in_file(&detached).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("title = {Relocated}"));
        assert!(written.starts_with("% inserted later"));
    }

    #[test]
    fn update_entry_reports_conflict_when_entry_disappears() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("library.bib");
        fs::write(
            &path,
            "@article{alpha,\n  title = {Alpha}\n}\n",
        )
        .unwrap();

        let bibliography = load_from_file(&path).unwrap();
        let entry = bibliography.get(&EntryId::from("alpha")).unwrap().clone();
        fs::write(
            &path,
            "@article{renamed,\n  title = {Alpha}\n}\n",
        )
        .unwrap();

        let error = BibStore::update_entry_in_file(&entry).unwrap_err();
        assert!(error.to_string().contains("unable to locate entry"));
    }

    #[test]
    fn ordered_fields_follow_original_field_order() {
        let entry = Entry {
            id: "alpha".into(),
            entry_type: "article".to_string(),
            fields: HashMap::from([
                ("year".to_string(), "2020".to_string()),
                ("title".to_string(), "Alpha".to_string()),
                ("author".to_string(), "Ada".to_string()),
            ]),
            provenance: crate::domain::Provenance {
                file_path: PathBuf::from("alpha.bib"),
                line_start: 1,
                line_end: 4,
                byte_start: 0,
                byte_end: 0,
            },
        };

        let rendered = render_entry(
            &entry,
            "@article{alpha,\n    author = {Ada},\n    title = {Alpha},\n    year = {2020}\n}\n",
        );

        assert_eq!(
            rendered,
            "@article{alpha,\n    author = {Ada},\n    title = {Alpha},\n    year = {2020}\n}\n"
        );
    }
}
