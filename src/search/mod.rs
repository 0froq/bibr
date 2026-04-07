use nucleo_matcher::{
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
    Matcher, Utf32Str,
};
use regex::Regex;

use crate::{
    config::{MatchMode, SearchConfig},
    domain::{Bibliography, EntryId},
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub terms: Vec<QueryTerm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryTerm {
    Plain(String),
    Field { field: String, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseSensitivity {
    Sensitive,
    Insensitive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub entry_id: EntryId,
    pub score: u32,
}

#[derive(Debug, Clone)]
pub struct SearchEngine {
    config: SearchConfig,
}

const EXACT_MATCH_SCORE: u32 = 100;
const SEARCH_FIELDS: [&str; 5] = ["author", "title", "year", "journal", "abstract"];

fn get_match_mode(config: &SearchConfig, field: &str) -> MatchMode {
    config
        .fuzzy
        .get(field)
        .copied()
        .unwrap_or(MatchMode::Substring)
}

impl SearchEngine {
    pub fn new(config: SearchConfig) -> Self {
        Self { config }
    }

    pub fn search(&self, bibliography: &Bibliography, query: &Query) -> Vec<SearchResult> {
        if query.terms.is_empty() {
            let mut results: Vec<_> = bibliography
                .iter()
                .map(|entry| SearchResult {
                    entry_id: entry.id.clone(),
                    score: 0,
                })
                .collect();
            sort_results(&mut results);
            return results;
        }

        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
        let mut utf32_buf = Vec::new();
        let mut results = Vec::new();

        for entry in bibliography.iter() {
            let mut total_score = 0;
            let mut all_terms_matched = true;

            for term in &query.terms {
                match term {
                    QueryTerm::Plain(text) => {
                        let mut best_score: Option<u32> = None;
                        for field_name in SEARCH_FIELDS {
                            if let Some(field_value) = entry.get_field(field_name) {
                                let mode = get_match_mode(&self.config, field_name);
                                if let Some(score) = self.match_with_mode(
                                    field_value,
                                    text.as_str(),
                                    &mut matcher,
                                    &mut utf32_buf,
                                    mode,
                                ) {
                                    best_score = Some(best_score.map_or(score, |s| s.max(score)));
                                }
                            }
                        }

                        if let Some(score) = best_score {
                            total_score += score;
                        } else {
                            all_terms_matched = false;
                            break;
                        }
                    }
                    QueryTerm::Field { field, text } => {
                        let Some(field_value) = entry.get_field(field) else {
                            all_terms_matched = false;
                            break;
                        };

                        let mode = get_match_mode(&self.config, field);
                        let Some(score) = self.match_with_mode(
                            &field_value,
                            text.as_str(),
                            &mut matcher,
                            &mut utf32_buf,
                            mode,
                        ) else {
                            all_terms_matched = false;
                            break;
                        };

                        total_score += score;
                    }
                }
            }

            if all_terms_matched {
                results.push(SearchResult {
                    entry_id: entry.id.clone(),
                    score: total_score,
                });
            }
        }

        sort_results(&mut results);
        results
    }

    fn match_with_mode(
        &self,
        haystack: &str,
        needle: &str,
        matcher: &mut Matcher,
        utf32_buf: &mut Vec<char>,
        mode: MatchMode,
    ) -> Option<u32> {
        if needle.trim().is_empty() {
            return Some(0);
        }

        let case_matching = match self.case_sensitivity(needle) {
            CaseSensitivity::Sensitive => CaseMatching::Respect,
            CaseSensitivity::Insensitive => CaseMatching::Ignore,
        };

        match mode {
            MatchMode::Exact => haystack
                .eq_ignore_ascii_case(needle)
                .then_some(EXACT_MATCH_SCORE),
            MatchMode::Fuzzy => {
                let pattern =
                    Pattern::new(needle, case_matching, Normalization::Smart, AtomKind::Fuzzy);
                pattern.score(Utf32Str::new(haystack, utf32_buf), matcher)
            }
            MatchMode::Substring => {
                let pattern = Pattern::new(
                    needle,
                    case_matching,
                    Normalization::Smart,
                    AtomKind::Substring,
                );
                pattern.score(Utf32Str::new(haystack, utf32_buf), matcher)
            }
        }
    }

    fn case_sensitivity(&self, query: &str) -> CaseSensitivity {
        if self.config.smart_case {
            smart_case(query)
        } else {
            CaseSensitivity::Insensitive
        }
    }
}

pub fn smart_case(query: &str) -> CaseSensitivity {
    if query.chars().any(char::is_uppercase) {
        CaseSensitivity::Sensitive
    } else {
        CaseSensitivity::Insensitive
    }
}

#[derive(Debug, Clone)]
struct QualifierSpan {
    start: usize,
    text_start: usize,
    field: String,
}

fn qualifier_spans(input: &str) -> Vec<QualifierSpan> {
    let regex = Regex::new(r"(^|\s)@([a-z][a-z0-9_-]*):").expect("valid qualifier regex");

    regex
        .captures_iter(input)
        .filter_map(|captures| {
            let full = captures.get(0)?;
            let field = captures.get(2)?.as_str().to_string();
            let start = full.start() + captures.get(1).map_or(0, |prefix| prefix.as_str().len());
            let text_start = full.end();

            Some(QualifierSpan {
                start,
                text_start,
                field,
            })
        })
        .collect()
}

fn trimmed(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn sort_results(results: &mut [SearchResult]) {
    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.entry_id.0.cmp(&right.entry_id.0))
    });
}

impl Query {
    pub fn parse(input: &str) -> Self {
        let qualifiers = qualifier_spans(input);

        if qualifiers.is_empty() {
            return Self {
                terms: trimmed(input)
                    .map(|text| vec![QueryTerm::Plain(text.to_string())])
                    .unwrap_or_default(),
            };
        }

        let mut terms = Vec::new();

        if let Some(first) = qualifiers.first() {
            if let Some(text) = trimmed(&input[..first.start]) {
                terms.push(QueryTerm::Plain(text.to_string()));
            }
        }

        for (index, qualifier) in qualifiers.iter().enumerate() {
            let text_end = qualifiers
                .get(index + 1)
                .map(|next| next.start)
                .unwrap_or(input.len());

            if let Some(text) = trimmed(&input[qualifier.text_start..text_end]) {
                terms.push(QueryTerm::Field {
                    field: qualifier.field.clone(),
                    text: text.to_string(),
                });
            }
        }

        Self { terms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;

    use crate::domain::load_from_file;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn load_fixture_bibliography() -> Bibliography {
        let mut bibliography = load_from_file(&fixture("test.bib")).unwrap();
        let second = load_from_file(&fixture("test2.bib")).unwrap();
        bibliography.merge(second).unwrap();
        bibliography
    }

    fn result_ids(results: Vec<SearchResult>) -> Vec<String> {
        results
            .into_iter()
            .map(|result| result.entry_id.0)
            .collect()
    }

    fn test_search_config() -> SearchConfig {
        let mut fuzzy = HashMap::new();
        fuzzy.insert("author".to_string(), MatchMode::Fuzzy);
        fuzzy.insert("title".to_string(), MatchMode::Substring);
        fuzzy.insert("year".to_string(), MatchMode::Substring);
        fuzzy.insert("journal".to_string(), MatchMode::Substring);
        fuzzy.insert("abstract".to_string(), MatchMode::Substring);

        SearchConfig {
            smart_case: true,
            fuzzy,
            search_all_fields: true,
        }
    }

    fn substring_search_config() -> SearchConfig {
        let mut fuzzy = HashMap::new();
        fuzzy.insert("author".to_string(), MatchMode::Substring);
        fuzzy.insert("title".to_string(), MatchMode::Substring);
        fuzzy.insert("year".to_string(), MatchMode::Substring);
        fuzzy.insert("journal".to_string(), MatchMode::Substring);
        fuzzy.insert("abstract".to_string(), MatchMode::Substring);

        SearchConfig {
            smart_case: true,
            fuzzy,
            search_all_fields: true,
        }
    }

    #[test]
    fn parses_plain_query() {
        assert_eq!(
            Query::parse("machine learning"),
            Query {
                terms: vec![QueryTerm::Plain("machine learning".to_string())],
            }
        );
    }

    #[test]
    fn parses_field_qualified_query() {
        assert_eq!(
            Query::parse("@title: machine learning"),
            Query {
                terms: vec![QueryTerm::Field {
                    field: "title".to_string(),
                    text: "machine learning".to_string(),
                }],
            }
        );
    }

    #[test]
    fn parses_multiple_qualified_terms() {
        assert_eq!(
            Query::parse("@author: smith @year: 2023"),
            Query {
                terms: vec![
                    QueryTerm::Field {
                        field: "author".to_string(),
                        text: "smith".to_string(),
                    },
                    QueryTerm::Field {
                        field: "year".to_string(),
                        text: "2023".to_string(),
                    },
                ],
            }
        );
    }

    #[test]
    fn parses_leading_plain_text_before_qualifier() {
        assert_eq!(
            Query::parse("compiler @year: 1952"),
            Query {
                terms: vec![
                    QueryTerm::Plain("compiler".to_string()),
                    QueryTerm::Field {
                        field: "year".to_string(),
                        text: "1952".to_string(),
                    },
                ],
            }
        );
    }

    #[test]
    fn smart_case_is_insensitive_for_lowercase_queries() {
        assert_eq!(smart_case("machine learning"), CaseSensitivity::Insensitive);
    }

    #[test]
    fn smart_case_is_sensitive_when_uppercase_is_present() {
        assert_eq!(smart_case("Machine learning"), CaseSensitivity::Sensitive);
    }

    #[test]
    fn empty_query_returns_all_entries() {
        let bibliography = load_fixture_bibliography();
        let engine = SearchEngine::new(test_search_config());

        let results = engine.search(&bibliography, &Query::parse("   "));

        assert_eq!(results.len(), bibliography.len());
        assert_eq!(
            result_ids(results),
            vec![
                "backus1978",
                "dijkstra1968",
                "hopper1952",
                "knuth1984",
                "mccarthy1960",
                "turing1936",
            ]
        );
    }

    #[test]
    fn plain_search_matches_real_entries() {
        let bibliography = load_fixture_bibliography();
        let engine = SearchEngine::new(substring_search_config());

        let results = engine.search(&bibliography, &Query::parse("literate"));

        assert_eq!(result_ids(results), vec!["knuth1984"]);
    }

    #[test]
    fn field_specific_search_only_checks_requested_field() {
        let bibliography = load_fixture_bibliography();
        let engine = SearchEngine::new(substring_search_config());

        let title_results = engine.search(&bibliography, &Query::parse("@title: computer"));
        let author_results = engine.search(&bibliography, &Query::parse("@author: computer"));

        assert_eq!(result_ids(title_results), vec!["hopper1952"]);
        assert!(author_results.is_empty());
    }

    #[test]
    fn combined_queries_use_and_logic() {
        let bibliography = load_fixture_bibliography();
        let engine = SearchEngine::new(test_search_config());

        let results = engine.search(&bibliography, &Query::parse("@author: knuth @year: 1984"));

        assert_eq!(result_ids(results), vec!["knuth1984"]);
    }

    #[test]
    fn results_are_ranked_by_relevance() {
        let bibliography = load_fixture_bibliography();
        let engine = SearchEngine::new(test_search_config());

        let results = engine.search(&bibliography, &Query::parse("programming"));
        let ids = result_ids(results);

        assert!(ids.contains(&"knuth1984".to_string()));
        assert!(ids.contains(&"backus1978".to_string()));
        assert!(!ids.is_empty());
    }

    #[test]
    fn smart_case_can_be_disabled_in_config() {
        let bibliography = load_fixture_bibliography();
        let mut config = test_search_config();
        config.smart_case = false;
        let engine = SearchEngine::new(config);

        let results = engine.search(&bibliography, &Query::parse("@author: KNUTH"));

        assert_eq!(result_ids(results), vec!["knuth1984"]);
    }

    #[test]
    fn author_field_uses_fuzzy_by_default() {
        let bibliography = load_fixture_bibliography();
        let engine = SearchEngine::new(test_search_config());
        assert_eq!(engine.config.fuzzy.get("author"), Some(&MatchMode::Fuzzy));
        let results = engine.search(&bibliography, &Query::parse("@author: Knuth"));
        assert_eq!(result_ids(results), vec!["knuth1984"]);
        let results = engine.search(&bibliography, &Query::parse("@author: Knu"));
        assert_eq!(result_ids(results), vec!["knuth1984"]);
    }

    #[test]
    fn title_field_uses_substring_by_default() {
        let bibliography = load_fixture_bibliography();
        let engine = SearchEngine::new(test_search_config());
        let results = engine.search(&bibliography, &Query::parse("@title: literate"));
        assert_eq!(result_ids(results), vec!["knuth1984"]);
        assert_eq!(
            engine.config.fuzzy.get("title"),
            Some(&MatchMode::Substring)
        );
    }

    #[test]
    fn disabling_fuzzy_search_falls_back_to_substring_matching() {
        let bibliography = load_fixture_bibliography();
        let default_engine = SearchEngine::new(test_search_config());
        let substring_engine = SearchEngine::new(substring_search_config());
        let query = Query::parse("programming");

        let default_results = default_engine.search(&bibliography, &query);
        let substring_results = substring_engine.search(&bibliography, &query);

        let default_ids = result_ids(default_results);
        let substring_ids = result_ids(substring_results);
        assert!(default_ids.contains(&"knuth1984".to_string()));
        assert!(substring_ids.contains(&"knuth1984".to_string()));
    }
}
