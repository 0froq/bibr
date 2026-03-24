use std::cmp::Ordering;

use super::Entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Year,
    Author,
    Title,
    Journal,
}

pub struct Sorter;

impl Sorter {
    pub fn sort(entries: &mut Vec<&Entry>, field: SortField, ascending: bool) {
        entries.sort_by(|left, right| compare_entries(left, right, field, ascending));
    }
}

fn compare_entries(left: &Entry, right: &Entry, field: SortField, ascending: bool) -> Ordering {
    let primary = match field {
        SortField::Year => compare_optional_u32(left.year(), right.year(), ascending),
        SortField::Author => compare_optional_text(first_author(left), first_author(right), ascending),
        SortField::Title => {
            compare_optional_text(left.title().map(normalized), right.title().map(normalized), ascending)
        }
        SortField::Journal => compare_optional_text(
            left.get_field("journal").map(normalized),
            right.get_field("journal").map(normalized),
            ascending,
        ),
    };

    primary.then_with(|| normalized(&left.id.0).cmp(&normalized(&right.id.0)))
}

fn first_author(entry: &Entry) -> Option<String> {
    entry.authors().into_iter().next().map(|author| normalized(&author))
}

fn compare_optional_u32(left: Option<u32>, right: Option<u32>, ascending: bool) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            if ascending {
                left.cmp(&right)
            } else {
                right.cmp(&left)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_text(left: Option<String>, right: Option<String>, ascending: bool) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            if ascending {
                left.cmp(&right)
            } else {
                right.cmp(&left)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn normalized(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::domain::{EntryId, Provenance};

    fn entry(
        citekey: &str,
        author: Option<&str>,
        title: Option<&str>,
        journal: Option<&str>,
        year: Option<&str>,
    ) -> Entry {
        let mut fields = HashMap::new();
        if let Some(author) = author {
            fields.insert("author".to_string(), author.to_string());
        }
        if let Some(title) = title {
            fields.insert("title".to_string(), title.to_string());
        }
        if let Some(journal) = journal {
            fields.insert("journal".to_string(), journal.to_string());
        }
        if let Some(year) = year {
            fields.insert("year".to_string(), year.to_string());
        }

        Entry {
            id: EntryId::from(citekey),
            entry_type: "article".to_string(),
            fields,
            provenance: Provenance {
                file_path: PathBuf::from("library.bib"),
                line_start: 1,
                line_end: 1,
                byte_start: 0,
                byte_end: 1,
            },
        }
    }

    #[test]
    fn sorts_by_year_with_missing_values_last() {
        let older = entry("older", Some("Knuth, Donald"), Some("A"), Some("Journal B"), Some("1984"));
        let newer = entry("newer", Some("Lamport, Leslie"), Some("B"), Some("Journal A"), Some("1994"));
        let missing = entry("missing", Some("Ada, Example"), Some("C"), None, None);
        let mut entries = vec![&missing, &newer, &older];

        Sorter::sort(&mut entries, SortField::Year, true);
        assert_eq!(entries.iter().map(|entry| entry.id.0.as_str()).collect::<Vec<_>>(), vec!["older", "newer", "missing"]);

        Sorter::sort(&mut entries, SortField::Year, false);
        assert_eq!(entries.iter().map(|entry| entry.id.0.as_str()).collect::<Vec<_>>(), vec!["newer", "older", "missing"]);
    }

    #[test]
    fn sorts_by_first_author() {
        let zebra = entry("zebra", Some("Zebra, Zoe and Able, Amy"), Some("A"), None, Some("2020"));
        let able = entry("able", Some("Able, Amy and Zebra, Zoe"), Some("B"), None, Some("2021"));
        let mut entries = vec![&zebra, &able];

        Sorter::sort(&mut entries, SortField::Author, true);

        assert_eq!(entries.iter().map(|entry| entry.id.0.as_str()).collect::<Vec<_>>(), vec!["able", "zebra"]);
    }

    #[test]
    fn sorts_by_journal_then_citekey() {
        let beta = entry("beta", None, Some("A"), Some("Beta Journal"), Some("2020"));
        let alpha = entry("alpha", None, Some("B"), Some("Alpha Journal"), Some("2020"));
        let alpha_two = entry("alpha-two", None, Some("C"), Some("Alpha Journal"), Some("2020"));
        let mut entries = vec![&beta, &alpha_two, &alpha];

        Sorter::sort(&mut entries, SortField::Journal, true);

        assert_eq!(
            entries.iter().map(|entry| entry.id.0.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "alpha-two", "beta"]
        );
    }
}
