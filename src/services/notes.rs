use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use regex::{Captures, Regex};
use tokio::fs;
use tokio::process::Command;

use crate::config::NotesConfig;
use crate::domain::Entry;

const DEFAULT_TEMPLATE: &str = r#"---
citekey: {citekey}
authors: {authors}
title: {title}
year: {year}
---

# Notes on {title}

## Summary

## Key Points

## References
"#;

const MAX_TITLE_LEN: usize = 48;

pub struct NotesService {
    config: NotesConfig,
}

impl NotesService {
    pub fn new(config: NotesConfig) -> Self {
        Self { config }
    }

    pub async fn create_or_open_note(&self, entry: &Entry) -> Result<PathBuf> {
        let path = self.ensure_note_exists(entry).await?;
        open_in_editor(&path).await?;
        Ok(path)
    }

    pub fn generate_note_content(&self, entry: &Entry) -> Result<String> {
        let template = match &self.config.template_file {
            Some(path) => std::fs::read_to_string(path)
                .with_context(|| format!("failed to read template '{}'", path.display()))?,
            None => DEFAULT_TEMPLATE.to_string(),
        };

        render_template(&template, entry)
    }

    pub fn note_path(&self, entry: &Entry) -> PathBuf {
        let rendered = render_filename_pattern(&self.config.filename_pattern, entry)
            .unwrap_or_else(|_| format!("{}.md", sanitize_filename_segment(&entry.id.0)));
        self.config.notes_dir.join(rendered)
    }

    pub async fn ensure_note_exists(&self, entry: &Entry) -> Result<PathBuf> {
        fs::create_dir_all(&self.config.notes_dir).await.with_context(|| {
            format!(
                "failed to create notes directory '{}'",
                self.config.notes_dir.display()
            )
        })?;

        let path = self.resolve_available_path(entry).await?;
        if fs::try_exists(&path).await? {
            return Ok(path);
        }

        let content = self.generate_note_content(entry)?;
        fs::write(&path, content)
            .await
            .with_context(|| format!("failed to write note '{}'", path.display()))?;

        Ok(path)
    }

    async fn resolve_available_path(&self, entry: &Entry) -> Result<PathBuf> {
        let preferred = self.note_path(entry);
        match fs::metadata(&preferred).await {
            Ok(metadata) if metadata.is_dir() => next_available_path(&preferred).await,
            Ok(_) => Ok(preferred),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(preferred),
            Err(error) => Err(error)
                .with_context(|| format!("failed to inspect note path '{}'", preferred.display())),
        }
    }
}

fn render_filename_pattern(pattern: &str, entry: &Entry) -> Result<String> {
    let regex = variable_regex();
    let mut rendered = String::new();
    let mut last = 0;

    for captures in regex.captures_iter(pattern) {
        let matched = captures.get(0).expect("pattern match should exist");
        rendered.push_str(&pattern[last..matched.start()]);
        let value = resolve_variable(&captures, entry, true)?;
        rendered.push_str(&sanitize_filename_segment(&value));
        last = matched.end();
    }

    rendered.push_str(&pattern[last..]);

    let file_name = rendered.trim();
    if file_name.is_empty() {
        bail!("note filename pattern rendered an empty filename");
    }

    Ok(normalize_rendered_filename(file_name))
}

fn render_template(template: &str, entry: &Entry) -> Result<String> {
    let regex = variable_regex();
    let mut output = String::new();
    let mut last = 0;

    for captures in regex.captures_iter(template) {
        let matched = captures.get(0).expect("pattern match should exist");
        output.push_str(&template[last..matched.start()]);
        output.push_str(&resolve_variable(&captures, entry, false)?);
        last = matched.end();
    }

    output.push_str(&template[last..]);
    Ok(output)
}

fn resolve_variable(captures: &Captures<'_>, entry: &Entry, filename_mode: bool) -> Result<String> {
    let variable = captures
        .get(1)
        .map(|value| value.as_str())
        .ok_or_else(|| anyhow!("template variable is missing a name"))?;
    let content = captures.get(2).map(|value| value.as_str());
    let default_value = captures.get(3).map(|value| value.as_str());

    let values = NoteValues::from_entry(entry);

    // Resolve the base value
    let value = if let Some(val) = values.get_value(variable, filename_mode) {
        val
    } else {
        // Try to get from entry's BibTeX fields (case-insensitive)
        let field_key = variable.to_ascii_lowercase();
        if let Some(field_value) = entry.get_field(&field_key) {
            Value::String(field_value.to_string())
        } else if let Some(default) = default_value {
            return Ok(default.to_string());
        } else {
            // Field doesn't exist and no default - keep the original placeholder
            return Ok(captures.get(0).map(|m| m.as_str()).unwrap_or(variable).to_string());
        }
    };

    // Parse content into format string and transforms
    let (format_str, transforms) = parse_content(content);

    // Apply format (for authors) → transforms chain
    apply_transforms_chain(variable, value, format_str, transforms, &values, filename_mode)
}

fn parse_content(content: Option<&str>) -> (Option<&str>, Vec<&str>) {
    let Some(content) = content else {
        return (None, Vec::new());
    };

    if content.is_empty() {
        return (None, Vec::new());
    }

    // Check if content starts with % (format string)
    if content.starts_with('%') {
        // Find the first : that separates format from transforms
        // But we need to be careful - format strings like %F %L shouldn't have colons
        // So we look for : followed by a transform (not part of the format)
        if let Some(pos) = content.find(':') {
            let format_str = &content[..pos];
            let transforms: Vec<&str> = content[pos + 1..].split(':').filter(|s| !s.is_empty()).collect();
            (Some(format_str), transforms)
        } else {
            // Entire content is a format string
            (Some(content), Vec::new())
        }
    } else {
        // No format string, everything is transforms
        let transforms: Vec<&str> = content.split(':').filter(|s| !s.is_empty()).collect();
        (None, transforms)
    }
}

fn apply_transforms_chain(
    variable: &str,
    value: Value,
    format_str: Option<&str>,
    transforms: Vec<&str>,
    values: &NoteValues,
    filename_mode: bool,
) -> Result<String> {
    // Step 1: Apply format string (for authors) or datetime format
    let mut current_value = value;

    if let Some(fmt) = format_str {
        if variable == "authors" {
            // Apply author format string to each author
            if let Value::List(_) = &current_value {
                current_value = apply_author_format(current_value, fmt);
            }
        } else if matches!(variable, "date" | "time" | "datetime") {
            // Datetime format - value is already a pre-formatted string
            // We re-format with the custom format string
            if let Value::String(_) = &current_value {
                let reformatted = values.format_datetime(fmt);
                current_value = Value::String(reformatted);
            }
        }
    }

    // Step 2: Apply each transform in sequence
    for transform in transforms {
        current_value = apply_single_transform(variable, current_value, transform, values, filename_mode)?;
    }

    Ok(current_value.to_string_val())
}

fn apply_author_format(value: Value, format_str: &str) -> Value {
    let Value::List(authors) = value else {
        return value;
    };

    let formatted: Vec<String> = authors
        .iter()
        .map(|author| format_single_author(author, format_str))
        .collect();

    Value::List(formatted)
}

fn format_single_author(author: &str, format_str: &str) -> String {
    let (first_names, last_name) = parse_author_name(author);

    // Replace %F with first names, %L with last name
    format_str
        .replace("%F", &first_names)
        .replace("%L", &last_name)
}

fn parse_author_name(author: &str) -> (String, String) {
    let author = author.trim();

    if author.is_empty() {
        return (String::new(), String::new());
    }

    // Check for "Last, First" format (BibTeX standard)
    if let Some((last, first)) = author.split_once(',') {
        let last = last.trim().to_string();
        let first = first.trim().to_string();
        return (first, last);
    }

    // Otherwise, assume "First Last" or "First Middle Last" format
    // Split and take the last word as surname
    let parts: Vec<&str> = author.split_whitespace().collect();
    if parts.len() >= 2 {
        let last = parts.last().unwrap().to_string();
        let first = parts[..parts.len() - 1].join(" ");
        return (first, last);
    }

    // Single name - treat as last name only
    (String::new(), author.to_string())
}

fn apply_single_transform(
    variable: &str,
    value: Value,
    transform: &str,
    values: &NoteValues,
    filename_mode: bool,
) -> Result<Value> {
    match transform {
        // List transforms
        t if t.starts_with("slice(") && t.ends_with(')') => {
            apply_list_slice(value, t)
        }
        t if t.starts_with("join(") && t.ends_with(')') => {
            let sep = &t[5..t.len() - 1];
            let sep = if sep.is_empty() { ", " } else { sep };
            Ok(Value::String(value.to_string_val().replace(", ", sep)))
        }
        "join" => {
            // Default join behavior
            if variable == "authors" {
                let joined = if filename_mode {
                    values.author_surnames.join("-")
                } else {
                    values.author_surnames.join(", ")
                };
                Ok(Value::String(joined))
            } else {
                Ok(Value::String(value.to_string_val()))
            }
        }
        // String transforms
        "lower" => Ok(Value::String(value.to_string_val().to_lowercase())),
        "upper" => Ok(Value::String(value.to_string_val().to_uppercase())),
        _ => bail!("unsupported transform '{transform}' for variable '{variable}'"),
    }
}

fn apply_list_slice(value: Value, transform: &str) -> Result<Value> {
    let Value::List(list) = value else {
        // For non-list values, treat as single-element list
        return Ok(value);
    };

    let args = &transform[6..transform.len() - 1];
    let (start_str, end_str) = args.split_once(',').unwrap_or((args, ""));

    let start: usize = start_str.trim().parse().unwrap_or(0);
    let end: usize = if end_str.trim().is_empty() {
        list.len()
    } else {
        end_str.trim().parse().unwrap_or(list.len())
    };

    let end = end.min(list.len());
    let start = start.min(end);

    Ok(Value::List(list[start..end].to_vec()))
}

fn apply_transform_to_value(
    variable: &str,
    value: Value,
    transform: Option<&str>,
    values: &NoteValues,
    filename_mode: bool,
) -> Result<String> {
    let Some(transform) = transform else {
        return Ok(value.to_string_val());
    };

    // Handle datetime transforms
    if matches!(variable, "date" | "time" | "datetime") {
        if let Value::String(s) = &value {
            return Ok(values.format_datetime(&s));
        }
    }

    // Handle join transform with separator for list values
    if transform.starts_with("join(") && transform.ends_with(')') {
        let sep = &transform[5..transform.len() - 1];
        let sep = if sep.is_empty() { ", " } else { sep };
        // Properly join list elements without string replacement
        if let Value::List(list) = &value {
            return Ok(list.join(sep));
        }
        return Ok(value.to_string_val());
    }

    // For list values, apply transform to each element or as a whole
    let s = value.to_string_val();
    match transform {
        "lower" => Ok(s.to_lowercase()),
        "upper" => Ok(s.to_uppercase()),
        "join" => {
            // Default join uses surnames for authors
            if variable == "authors" {
                let joined = if filename_mode {
                    values.author_surnames.join("-")
                } else {
                    values.author_surnames.join(", ")
                };
                Ok(joined)
            } else {
                Ok(s)
            }
        }
        _ if transform.starts_with("slice(") && transform.ends_with(')') => {
            apply_slice_transform(&s, transform)
        }
        _ => bail!("unsupported transform '{transform}' for variable '{variable}'"),
    }
}

fn apply_slice_transform(value: &str, transform: &str) -> Result<String> {
    let args = &transform[6..transform.len() - 1];
    let (start, end) = args
        .split_once(',')
        .ok_or_else(|| anyhow!("slice transform must be formatted as slice(start,end)"))?;
    let start: usize = start.trim().parse().context("invalid slice start")?;
    let end: usize = end.trim().parse().context("invalid slice end")?;

    let chars = value.chars().collect::<Vec<_>>();
    if start >= chars.len() || start >= end {
        return Ok(String::new());
    }

    let end = end.min(chars.len());
    Ok(chars[start..end].iter().collect())
}

fn normalize_rendered_filename(file_name: &str) -> String {
    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("note");
    let extension = path.extension().and_then(|value| value.to_str());

    let normalized_stem = sanitize_filename_segment(stem);
    match extension {
        Some(extension) if !extension.is_empty() => {
            format!("{normalized_stem}.{}", sanitize_filename_segment(extension))
        }
        _ => normalized_stem,
    }
}

fn sanitize_filename_segment(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_dash = false;

    for ch in value.trim().chars() {
        let valid = ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@');
        if valid {
            output.push(ch);
            last_was_dash = false;
            continue;
        }

        if !last_was_dash {
            output.push('-');
            last_was_dash = true;
        }
    }

    let trimmed = output.trim_matches(['-', '.', ' ']);
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "note".to_string()
    } else {
        trimmed.to_string()
    }
}

fn variable_regex() -> Regex {
    // Pattern: {var} or {var:content} or {var/default}
    // The content after : is parsed separately for formats and transforms
    Regex::new(r"\{([a-zA-Z_][a-zA-Z0-9_]*)(?::([^}/]*))?(?:/([^}]*))?\}")
        .expect("notes regex should be valid")
}

async fn next_available_path(preferred: &Path) -> Result<PathBuf> {
    let parent = preferred
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = preferred
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("note");
    let extension = preferred.extension().and_then(|value| value.to_str());

    for index in 1..=9_999 {
        let candidate_name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = parent.join(candidate_name);
        if !fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
    }

    bail!(
        "unable to resolve a unique note path for '{}'",
        preferred.display()
    )
}

async fn open_in_editor(path: &Path) -> Result<()> {
    let command = editor_command().unwrap_or_else(default_open_command);
    let Some((program, args)) = command.split_first() else {
        bail!("no editor command available to open '{}'", path.display());
    };

    let status = Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .await
        .with_context(|| format!("failed to launch editor '{}'", program))?;

    if !status.success() {
        bail!(
            "editor '{}' exited with status {} while opening '{}'",
            program,
            status,
            path.display()
        );
    }

    Ok(())
}

fn editor_command() -> Option<Vec<String>> {
    std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|value| !value.trim().is_empty()))
        .map(|value| split_command_line(&value))
        .filter(|parts| !parts.is_empty())
}

fn default_open_command() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        return vec!["open".to_string(), "-t".to_string()];
    }

    #[cfg(target_os = "windows")]
    {
        return vec!["cmd".to_string(), "/C".to_string(), "start".to_string()];
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec!["xdg-open".to_string()]
    }
}

fn split_command_line(command: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;

    for ch in command.chars() {
        match (quote, ch) {
            (Some(active), next) if next == active => quote = None,
            (Some(_), next) => current.push(next),
            (None, '"' | '\'') => quote = Some(ch),
            (None, next) if next.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            (None, next) => current.push(next),
        }
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

enum Value {
    String(String),
    List(Vec<String>),
}

impl Value {
    fn to_string_val(self) -> String {
        match self {
            Value::String(s) => s,
            Value::List(l) => l.join(", "),
        }
    }
}

struct NoteValues {
    citekey: String,
    authors: Vec<String>,
    author_surnames: Vec<String>,
    title: String,
    filename_title: String,
    year: String,
    datetime: chrono::DateTime<Local>,
}

impl NoteValues {
    fn from_entry(entry: &Entry) -> Self {
        // Store original author names as they appear in BibTeX
        let authors: Vec<String> = entry
            .authors()
            .into_iter()
            .map(|author| author.trim().to_string())
            .filter(|author| !author.is_empty())
            .collect();

        // Compute surnames from the original authors
        let author_surnames = authors
            .iter()
            .map(|author| extract_surname(author))
            .filter(|surname| !surname.is_empty())
            .collect();

        let title = entry.title().unwrap_or("Untitled").trim().to_string();

        Self {
            citekey: entry.id.0.clone(),
            authors,
            author_surnames,
            filename_title: truncate_chars(&title, MAX_TITLE_LEN),
            title,
            year: entry
                .year()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "Unknown".to_string()),
            datetime: Local::now(),
        }
    }

    fn get_value(&self, variable: &str, filename_mode: bool) -> Option<Value> {
        match variable {
            "citekey" => Some(Value::String(self.citekey.clone())),
            "authors" => Some(Value::List(self.authors.clone())),
            "title" if filename_mode => Some(Value::String(self.filename_title.clone())),
            "title" => Some(Value::String(self.title.clone())),
            "filename_title" => Some(Value::String(self.filename_title.clone())),
            "year" => Some(Value::String(self.year.clone())),
            "date" => Some(Value::String(self.datetime.format("%Y-%m-%d").to_string())),
            "time" => Some(Value::String(self.datetime.format("%H:%M:%S").to_string())),
            "datetime" => Some(Value::String(
                self.datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
            )),
            _ => None,
        }
    }

    fn format_datetime(&self, format: &str) -> String {
        self.datetime.format(format).to_string()
    }
}

fn normalize_author_name(author: &str) -> String {
    let author = author.trim();
    if author.is_empty() {
        return String::new();
    }

    if author.contains(',') {
        return author.to_string();
    }

    let parts: Vec<&str> = author.split_whitespace().collect();
    if parts.len() >= 2 {
        let surname = parts.last().unwrap();
        let first_names = &parts[..parts.len() - 1].join(" ");
        format!("{}, {}", surname, first_names)
    } else {
        author.to_string()
    }
}

fn extract_surname(author: &str) -> String {
    let author = author.trim();
    if author.is_empty() {
        return String::new();
    }

    if let Some((surname, _)) = author.split_once(',') {
        return surname.trim().to_string();
    }

    author
        .split_whitespace()
        .last()
        .unwrap_or(author)
        .trim()
        .to_string()
}

fn truncate_chars(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::tempdir;

    use super::*;
    use crate::domain::{Entry, EntryId, Provenance};

    #[test]
    fn note_path_uses_default_pattern() {
        let temp = tempdir().expect("temp dir should be created");
        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().to_path_buf(),
            filename_pattern: "{citekey}.md".to_string(),
            template_file: None,
        });

        let path = service.note_path(&sample_entry());

        assert_eq!(path, temp.path().join("Doe2024.md"));
    }

    #[test]
    fn note_path_supports_transforms_and_patterns() {
        let temp = tempdir().expect("temp dir should be created");
        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().to_path_buf(),
            filename_pattern: "{citekey:lower}-{authors:%L:slice(0,3)}-{year}-{date:%Y-%m-%d}.md".to_string(),
            template_file: None,
        });

        let path = service.note_path(&sample_entry());
        let today = Local::now().format("%Y-%m-%d").to_string();

        assert_eq!(path, temp.path().join(format!("doe2024-Doe-Smith-2024-{today}.md")));
    }

    #[test]
    fn note_path_joins_multiple_author_surnames() {
        let temp = tempdir().expect("temp dir should be created");
        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().to_path_buf(),
            filename_pattern: "{authors:join}.md".to_string(),
            template_file: None,
        });

        let path = service.note_path(&sample_entry());

        assert_eq!(path, temp.path().join("Doe-Smith.md"));
    }

    #[test]
    fn note_path_truncates_and_sanitizes_title() {
        let temp = tempdir().expect("temp dir should be created");
        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().to_path_buf(),
            filename_pattern: "{title}.md".to_string(),
            template_file: None,
        });
        let mut entry = sample_entry();
        entry.set_field(
            "title",
            "A Very Long Title With / Forbidden Characters: And More Than Forty Eight Characters",
        );

        let path = service.note_path(&entry);

        assert_eq!(
            path,
            temp.path()
                .join("A-Very-Long-Title-With-Forbidden-Characters-A.md")
        );
    }

    #[test]
    fn generate_note_content_substitutes_template_variables() {
        let temp = tempdir().expect("temp dir should be created");
        let template_path = temp.path().join("template.md");
        std::fs::write(
            &template_path,
            "citekey={citekey}\nauthors={authors:join}\ntitle={title}\nyear={year}\n",
        )
        .expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().join("notes"),
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_path),
        });

        let content = service
            .generate_note_content(&sample_entry())
            .expect("template should render");

        assert!(content.contains("citekey=Doe2024"));
        assert!(content.contains("authors=Doe, Smith"));
        assert!(content.contains("title=Understanding Note Systems"));
        assert!(content.contains("year=2024"));
    }

    #[tokio::test]
    async fn ensure_note_exists_creates_note_only_on_demand() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        let service = NotesService::new(NotesConfig {
            notes_dir: notes_dir.clone(),
            filename_pattern: "{citekey}.md".to_string(),
            template_file: None,
        });
        let expected_path = notes_dir.join("Doe2024.md");

        assert!(!expected_path.exists());

        let created = service
            .ensure_note_exists(&sample_entry())
            .await
            .expect("note should be created");

        assert_eq!(created, expected_path);
        assert!(expected_path.exists());

        let original = std::fs::read_to_string(&expected_path).expect("note should exist");
        std::fs::write(&expected_path, "custom content").expect("note should be updated");

        let reopened = service
            .ensure_note_exists(&sample_entry())
            .await
            .expect("existing note should be reused");

        assert_eq!(reopened, expected_path);
        assert_eq!(
            std::fs::read_to_string(&expected_path).expect("note should still exist"),
            "custom content"
        );
        assert!(original.contains("# Notes on Understanding Note Systems"));
    }

    #[test]
    fn note_path_uses_arbitrary_bibtex_fields() {
        let temp = tempdir().expect("temp dir should be created");
        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().to_path_buf(),
            filename_pattern: "{citekey}-{journal}.md".to_string(),
            template_file: None,
        });

        // Entry with journal field
        let mut entry = sample_entry();
        entry.fields.insert("journal".to_string(), "Nature".to_string());

        let path = service.note_path(&entry);

        assert_eq!(path, temp.path().join("Doe2024-Nature.md"));
    }

    #[test]
    fn note_path_keeps_placeholder_for_missing_fields() {
        let temp = tempdir().expect("temp dir should be created");
        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().to_path_buf(),
            filename_pattern: "{citekey}-{publisher}.md".to_string(),
            template_file: None,
        });

        // Entry without publisher field
        let entry = sample_entry();

        let path = service.note_path(&entry);

        // Placeholder is kept but sanitized for filename (curly braces become dashes)
        assert!(path.to_string_lossy().contains("publisher"));
    }

    #[test]
    fn note_path_uses_default_value_for_missing_fields() {
        let temp = tempdir().expect("temp dir should be created");
        let service = NotesService::new(NotesConfig {
            notes_dir: temp.path().to_path_buf(),
            filename_pattern: "{citekey}-{publisher/unknown}.md".to_string(),
            template_file: None,
        });

        // Entry without publisher field - should use default
        let entry = sample_entry();

        let path = service.note_path(&entry);

        assert_eq!(path, temp.path().join("Doe2024-unknown.md"));
    }

    #[test]
    fn template_substitutes_arbitrary_bibtex_fields() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Published in: {journal}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        let mut entry = sample_entry();
        entry.fields.insert("journal".to_string(), "Science".to_string());

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Published in: Science"));
    }

    #[test]
    fn template_keeps_placeholder_for_missing_fields() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Volume: {volume}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        // Entry without volume field
        let entry = sample_entry();

        let content = service.generate_note_content(&entry).expect("content should be generated");

        // Should keep the placeholder
        assert!(content.contains("Volume: {volume}"));
    }

    #[test]
    fn template_uses_default_value_for_missing_fields() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Volume: {volume/N/A}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        // Entry without volume field - should use default
        let entry = sample_entry();

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Volume: N/A"));
    }

    #[test]
    fn template_uses_field_value_when_exists_despite_default() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Volume: {volume/N/A}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        // Entry WITH volume field - should use actual value, not default
        let mut entry = sample_entry();
        entry.fields.insert("volume".to_string(), "42".to_string());

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Volume: 42"));
        assert!(!content.contains("N/A"));
    }

    #[test]
    fn template_authors_format_first_last() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Authors: {authors:%F %L}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        let entry = sample_entry();

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Authors: Jane Doe, John Smith"));
    }

    #[test]
    fn template_authors_format_last_first() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Authors: {authors:%L, %F}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        let entry = sample_entry();

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Authors: Doe, Jane, Smith, John"));
    }

    #[test]
    fn template_authors_slice_first_three() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Authors: {authors:slice(0,3)}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        let mut entry = sample_entry();
        entry.fields.insert("author".to_string(), "Doe, Jane and Smith, John and Brown, Alice".to_string());

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Authors: Doe, Jane, Smith, John, Brown, Alice"));
    }

    #[test]
    fn template_authors_join_with_separator() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Authors: {authors:%F %L:join( and )}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        let entry = sample_entry();

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Authors: Jane Doe and John Smith"));
    }

    #[test]
    fn template_authors_slice_and_join() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "First two: {authors:slice(0,2):join(, )}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        let mut entry = sample_entry();
        entry.fields.insert("author".to_string(), "Doe, Jane and Smith, John and Brown, Alice".to_string());

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("First two: Doe, Jane, Smith, John"));
    }

    #[test]
    fn template_authors_full_chain_example() {
        let temp = tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        std::fs::create_dir(&notes_dir).expect("notes dir should be created");

        let template_file = temp.path().join("template.md");
        std::fs::write(&template_file, "Authors: {authors:%F %L:slice(0,3):join(, ):lower}").expect("template should be written");

        let service = NotesService::new(NotesConfig {
            notes_dir,
            filename_pattern: "{citekey}.md".to_string(),
            template_file: Some(template_file),
        });

        let mut entry = sample_entry();
        entry.fields.insert("author".to_string(), "Doe, Jane and Smith, John and Brown, Alice".to_string());

        let content = service.generate_note_content(&entry).expect("content should be generated");

        assert!(content.contains("Authors: jane doe, john smith, alice brown"));
    }

    fn sample_entry() -> Entry {
        let mut fields = HashMap::new();
        fields.insert("author".to_string(), "Doe, Jane and John Smith".to_string());
        fields.insert("title".to_string(), "Understanding Note Systems".to_string());
        fields.insert("year".to_string(), "2024".to_string());

        Entry {
            id: EntryId("Doe2024".to_string()),
            entry_type: "article".to_string(),
            fields,
            provenance: Provenance {
                file_path: PathBuf::from("library.bib"),
                line_start: 1,
                line_end: 1,
                byte_start: 0,
                byte_end: 0,
            },
        }
    }
}
