pub mod sort;

use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use biblatex::ChunksExt;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, LoadError>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntryId(pub String);

impl Display for EntryId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for EntryId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for EntryId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub file_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    pub entry_type: String,
    pub fields: HashMap<String, String>,
    pub provenance: Provenance,
}

impl Entry {
    pub fn get_field(&self, key: &str) -> Option<&str> {
        self.fields
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn authors(&self) -> Vec<String> {
        self.get_field("author")
            .map(|authors| {
                authors
                    .split(" and ")
                    .map(str::trim)
                    .filter(|author| !author.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn title(&self) -> Option<&str> {
        self.get_field("title")
    }

    pub fn year(&self) -> Option<u32> {
        self.get_field("year")?.trim().parse().ok()
    }

    pub fn set_field(&mut self, key: &str, value: impl Into<String>) {
        self.fields.insert(key.to_ascii_lowercase(), value.into());
    }
}

#[derive(Debug, Clone, Default)]
pub struct Bibliography {
    pub entries: HashMap<EntryId, Entry>,
    pub sources: Vec<PathBuf>,
}

impl Bibliography {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_entry(&mut self, entry: Entry) -> std::result::Result<(), DuplicateError> {
        if self.entries.contains_key(&entry.id) {
            return Err(DuplicateError {
                id: entry.id.clone(),
                existing_path: self.entries[&entry.id].provenance.file_path.clone(),
                duplicate_path: entry.provenance.file_path.clone(),
            });
        }

        self.push_source(&entry.provenance.file_path);
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }

    pub fn get(&self, id: &EntryId) -> Option<&Entry> {
        self.entries.get(id)
    }

    pub fn get_mut(&mut self, id: &EntryId) -> Option<&mut Entry> {
        self.entries.get_mut(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub fn merge(&mut self, other: Bibliography) -> std::result::Result<(), MergeError> {
        for entry in other.entries.values() {
            if let Some(existing) = self.entries.get(&entry.id) {
                return Err(MergeError::Duplicate(DuplicateError {
                    id: entry.id.clone(),
                    existing_path: existing.provenance.file_path.clone(),
                    duplicate_path: entry.provenance.file_path.clone(),
                }));
            }
        }

        for source in other.sources {
            self.push_source(&source);
        }

        for (id, entry) in other.entries {
            self.entries.insert(id, entry);
        }

        Ok(())
    }

    fn push_source(&mut self, source: &Path) {
        if !self.sources.iter().any(|existing| existing == source) {
            self.sources.push(source.to_path_buf());
        }
    }

    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        let mut entries = self.entries.values().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id.0.cmp(&right.id.0));

        let mut output = String::new();
        for (index, entry) in entries.iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }

            output.push_str(&format_entry(entry));
        }

        let mut file = fs::File::create(path)?;
        file.write_all(output.as_bytes())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error(
    "duplicate citekey `{id}` found in `{duplicate_path}`; already loaded from `{existing_path}`"
)]
pub struct DuplicateError {
    pub id: EntryId,
    pub existing_path: PathBuf,
    pub duplicate_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum MergeError {
    #[error(transparent)]
    Duplicate(#[from] DuplicateError),
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("failed to read `{path}`")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Duplicate(#[from] DuplicateError),
}

pub fn load_from_files(paths: &[PathBuf]) -> Result<Bibliography> {
    let mut bibliography = Bibliography::new();

    for path in paths {
        let loaded = load_from_file(path)?;
        bibliography.merge(loaded).map_err(|error| match error {
            MergeError::Duplicate(error) => LoadError::Duplicate(error),
        })?;
    }

    Ok(bibliography)
}

#[derive(Debug, Clone)]
pub struct LoadWarning {
    pub line_start: usize,
    pub line_end: usize,
    pub citekey: Option<String>,
    pub message: String,
}

pub struct LoadResult {
    pub bibliography: Bibliography,
    pub warnings: Vec<LoadWarning>,
    pub total_blocks: usize,
    pub parsed_entries: usize,
}

pub fn load_from_file_with_diagnostics(path: &Path) -> Result<LoadResult> {
    let source = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let file_path = path.to_path_buf();
    let mut bibliography = Bibliography::new();
    bibliography.push_source(&file_path);

    let mut abbreviations = String::new();
    let mut warnings = Vec::new();
    let mut total_blocks = 0;
    let mut parsed_entries = 0;

    for block in scan_blocks(&source) {
        total_blocks += 1;
        
        match block.kind {
            BlockKind::String => {
                abbreviations.push_str(block.text);
                abbreviations.push('\n');
            }
            BlockKind::Entry => {
                let line_start = line_number_at(&source, block.start);
                let line_end = line_number_at(&source, block.end.saturating_sub(1));
                
                match parse_entry_block(block.text, &abbreviations) {
                    Ok(Some(entry)) => {
                        let provenance = Provenance {
                            file_path: file_path.clone(),
                            line_start,
                            line_end,
                            byte_start: block.start,
                            byte_end: block.end,
                        };

                        let entry_id = EntryId::from(entry.key.clone());
                        
                        if bibliography.entries.contains_key(&entry_id) {
                            let existing = bibliography.entries.get(&entry_id).unwrap();
                            return Err(LoadError::Duplicate(DuplicateError {
                                id: entry_id,
                                existing_path: existing.provenance.file_path.clone(),
                                duplicate_path: file_path.clone(),
                            }));
                        }
                        
                        bibliography.add_entry(Entry {
                            id: entry_id,
                            entry_type: entry.entry_type.to_string(),
                            fields: entry
                                .fields
                                .iter()
                                .map(|(key, value)| {
                                    (key.to_ascii_lowercase(), value.format_verbatim())
                                })
                                .collect(),
                            provenance,
                        })?;
                        parsed_entries += 1;
                    }
                    Ok(None) => {
                        warnings.push(LoadWarning {
                            line_start,
                            line_end,
                            citekey: None,
                            message: "Entry parsed but returned no data".to_string(),
                        });
                    }
                    Err(error) => {
                        let citekey = extract_citekey_from_block(block.text);
                        warnings.push(LoadWarning {
                            line_start,
                            line_end,
                            citekey,
                            message: format!("Parse error: {}", error),
                        });
                    }
                }
            }
            BlockKind::Comment | BlockKind::Preamble => {}
        }
    }

    Ok(LoadResult {
        bibliography,
        warnings,
        total_blocks,
        parsed_entries,
    })
}

pub fn load_from_file(path: &Path) -> Result<Bibliography> {
    load_from_file_with_diagnostics(path).map(|r| r.bibliography)
}

fn extract_citekey_from_block(text: &str) -> Option<String> {
    text.lines().next().and_then(|first_line| {
        let start = first_line.find('{')? + 1;
        let end = first_line[start..].find(',').map(|i| start + i)
            .or_else(|| first_line[start..].find('}').map(|i| start + i))?;
        Some(first_line[start..end].trim().to_string())
    })
}

fn parse_entry_block(
    entry_text: &str,
    abbreviations: &str,
) -> std::result::Result<Option<biblatex::Entry>, biblatex::ParseError> {
    let combined = if abbreviations.is_empty() {
        entry_text.to_string()
    } else {
        format!("{abbreviations}\n{entry_text}")
    };

    let bibliography = biblatex::Bibliography::parse(&combined)?;
    Ok(bibliography.into_vec().into_iter().next())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Entry,
    String,
    Comment,
    Preamble,
}

#[derive(Debug, Clone, Copy)]
struct Block<'a> {
    kind: BlockKind,
    start: usize,
    end: usize,
    text: &'a str,
}

fn scan_blocks(source: &str) -> Vec<Block<'_>> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut blocks = Vec::new();

    while index < bytes.len() {
        if bytes[index] == b'%' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }

        if bytes[index] != b'@' {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        let kind_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }

        let kind_text = source[kind_start..index].to_ascii_lowercase();

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        if index >= bytes.len() || !matches!(bytes[index], b'{' | b'(') {
            continue;
        }

        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;

        if bytes[index] == b'{' {
            brace_depth = 1;
        } else {
            paren_depth = 1;
        }

        index += 1;

        while index < bytes.len() {
            let byte = bytes[index];

            match byte {
                b'{' => brace_depth += 1,
                b'}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                }
                b'(' => paren_depth += 1,
                b')' => {
                    paren_depth = paren_depth.saturating_sub(1);
                }
                _ => {}
            }

            index += 1;

            if brace_depth == 0 && paren_depth == 0 {
                break;
            }
        }

        let end = index.min(bytes.len());
        let kind = match kind_text.as_str() {
            "string" => BlockKind::String,
            "comment" => BlockKind::Comment,
            "preamble" => BlockKind::Preamble,
            _ => BlockKind::Entry,
        };

        blocks.push(Block { kind, start, end, text: &source[start..end] });
    }

    blocks
}

fn line_number_at(source: &str, byte_index: usize) -> usize {
    let clamped = byte_index.min(source.len());
    source[..clamped].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn format_entry(entry: &Entry) -> String {
    let mut fields = entry.fields.iter().collect::<Vec<_>>();
    fields.sort_by(|left, right| left.0.cmp(right.0));

    let mut rendered = format!("@{}{{{},\n", entry.entry_type, entry.id);
    for (index, (key, value)) in fields.iter().enumerate() {
        rendered.push_str(&format!("  {} = {{{}}}", key, value));
        if index + 1 < fields.len() {
            rendered.push(',');
        }
        rendered.push('\n');
    }
    rendered.push('}');
    rendered.push('\n');

    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    fn provenance() -> Provenance {
        Provenance {
            file_path: PathBuf::from("tests/fixtures/test.bib"),
            line_start: 1,
            line_end: 10,
            byte_start: 0,
            byte_end: 128,
        }
    }

    #[test]
    fn entry_creation_and_field_access_work() {
        let entry = Entry {
            id: EntryId::from("knuth1984"),
            entry_type: "article".to_string(),
            fields: HashMap::from([
                ("title".to_string(), "Literate Programming".to_string()),
                (
                    "author".to_string(),
                    "Knuth, Donald E. and Lamport, Leslie".to_string(),
                ),
                ("year".to_string(), "1984".to_string()),
            ]),
            provenance: provenance(),
        };

        assert_eq!(entry.get_field("title"), Some("Literate Programming"));
        assert_eq!(
            entry.authors(),
            vec!["Knuth, Donald E.".to_string(), "Lamport, Leslie".to_string()]
        );
        assert_eq!(entry.title(), Some("Literate Programming"));
        assert_eq!(entry.year(), Some(1984));
    }

    #[test]
    fn bibliography_add_and_merge_work() {
        let mut left = Bibliography::new();
        let mut right = Bibliography::new();

        let entry_a = Entry {
            id: EntryId::from("alpha"),
            entry_type: "article".to_string(),
            fields: HashMap::from([("title".to_string(), "Alpha".to_string())]),
            provenance: Provenance {
                file_path: PathBuf::from("left.bib"),
                line_start: 1,
                line_end: 1,
                byte_start: 0,
                byte_end: 10,
            },
        };

        let entry_b = Entry {
            id: EntryId::from("beta"),
            entry_type: "book".to_string(),
            fields: HashMap::from([("title".to_string(), "Beta".to_string())]),
            provenance: Provenance {
                file_path: PathBuf::from("right.bib"),
                line_start: 1,
                line_end: 1,
                byte_start: 0,
                byte_end: 10,
            },
        };

        left.add_entry(entry_a).unwrap();
        right.add_entry(entry_b).unwrap();
        left.merge(right).unwrap();

        assert_eq!(left.len(), 2);
        assert!(left.get(&EntryId::from("alpha")).is_some());
        assert!(left.get(&EntryId::from("beta")).is_some());
        assert_eq!(left.sources.len(), 2);
    }

    #[test]
    fn loading_real_bibtex_file_extracts_entries() {
        let bibliography = load_from_file(&fixture("test.bib")).unwrap();

        assert_eq!(bibliography.len(), 4);

        let entry = bibliography.get(&EntryId::from("knuth1984")).unwrap();
        assert_eq!(entry.entry_type, "article");
        assert_eq!(entry.title(), Some("Literate Programming"));
        assert_eq!(entry.year(), Some(1984));
        assert_eq!(entry.get_field("journal"), Some("The Computer Journal"));
        assert_eq!(entry.get_field("doi"), Some("10.1093/comjnl/27.2.97"));
        assert_eq!(entry.provenance.file_path, fixture("test.bib"));
        assert!(entry.provenance.line_start < entry.provenance.line_end);
    }

    #[test]
    fn duplicate_citekeys_are_rejected() {
        let dir = tempdir().unwrap();
        let duplicate_path = dir.path().join("duplicate.bib");
        let source = r#"
@article{dupkey,
  title = {First},
  author = {One, Author},
  year = {2020}
}

@book{dupkey,
  title = {Second},
  author = {Two, Author},
  year = {2021}
}
"#;

        fs::write(&duplicate_path, source).unwrap();

        let error = load_from_file(&duplicate_path).unwrap_err();
        assert!(matches!(error, LoadError::Duplicate(_)));
    }

    #[test]
    fn malformed_entries_are_skipped_with_warning() {
        let dir = tempdir().unwrap();
        let malformed_path = dir.path().join("malformed.bib");
        let source = r#"
@article{validone,
  title = {Valid Entry},
  author = {Doe, Jane and Smith, John},
  year = {2022}
}

@article{broken,
  title = {Broken Entry},
  author = {Nobody, Example},
  year =
}

@book{validtwo,
  title = {Another Valid Entry},
  author = {Roe, Richard},
  year = {2023},
  url = {https://example.com/book}
}
"#;

        fs::write(&malformed_path, source).unwrap();

        let bibliography = load_from_file(&malformed_path).unwrap();

        assert_eq!(bibliography.len(), 2);
        assert!(bibliography.get(&EntryId::from("validone")).is_some());
        assert!(bibliography.get(&EntryId::from("validtwo")).is_some());
        assert!(bibliography.get(&EntryId::from("broken")).is_none());
    }

    #[test]
    fn fixture_files_merge_without_duplicates() {
        let dir = tempdir().unwrap();
        let unique_path = dir.path().join("unique.bib");
        fs::write(
            &unique_path,
            r#"
@article{dijkstra1968,
  title = {Go To Statement Considered Harmful},
  author = {Dijkstra, Edsger W.},
  year = {1968}
}

@book{backus1978,
  title = {Can Programming Be Liberated from the von Neumann Style?},
  author = {Backus, John},
  year = {1978}
}
"#,
        )
        .unwrap();

        let mut bibliography = load_from_file(&fixture("test.bib")).unwrap();
        let second = load_from_file(&unique_path).unwrap();

        bibliography.merge(second).unwrap();

        assert_eq!(bibliography.len(), 6);
        assert_eq!(bibliography.sources.len(), 2);
        assert!(bibliography.iter().any(|entry| entry.id == EntryId::from("mccarthy1960")));
    }

    #[test]
    fn loading_multiple_files_without_duplicates_succeeds() {
        let bibliography = load_from_files(&[fixture("test.bib"), fixture("test2.bib")]).unwrap();
        assert_eq!(bibliography.len(), 6);
    }

    #[test]
    fn saving_bibliography_roundtrips_updated_entries() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("roundtrip.bib");
        let mut bibliography = load_from_file(&fixture("test.bib")).unwrap();

        bibliography
            .get_mut(&EntryId::from("hopper1952"))
            .unwrap()
            .set_field("note", "Compiler history");
        bibliography.save_to_file(&output).unwrap();

        let reloaded = load_from_file(&output).unwrap();
        assert_eq!(reloaded.len(), 4);
        assert_eq!(
            reloaded
                .get(&EntryId::from("hopper1952"))
                .unwrap()
                .get_field("note"),
            Some("Compiler history")
        );
    }
}
