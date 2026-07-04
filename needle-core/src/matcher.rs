//! Evaluate a parsed Query against Index entries.

use crate::index::{contains_ignore_case, Entry, Index};
use crate::query::{Query, RangeFilter, TextTerm};
use regex::Regex;

struct CompiledTerms {
    items: Vec<CompiledTerm>,
}

enum CompiledTerm {
    Substring(String),
    Wildcard(String),
    Regex(Regex),
    Not(Box<CompiledTerm>),
    Or(Vec<CompiledTerm>),
}

fn compile_terms(terms: &[TextTerm]) -> CompiledTerms {
    let items = terms
        .iter()
        .map(|term| match term {
            TextTerm::Substring(s) => CompiledTerm::Substring(s.clone()),
            TextTerm::Wildcard(p) => CompiledTerm::Wildcard(p.clone()),
            TextTerm::Regex(p) => {
                let re = Regex::new(p).unwrap_or_else(|_| Regex::new(&regex::escape(p)).unwrap());
                CompiledTerm::Regex(re)
            }
            TextTerm::Not(inner) => CompiledTerm::Not(Box::new(
                compile_terms(&[inner.as_ref().clone()])
                    .items
                    .into_iter()
                    .next()
                    .unwrap(),
            )),
            TextTerm::Or(items) => CompiledTerm::Or(compile_terms(items).items),
        })
        .collect();
    CompiledTerms { items }
}

pub fn match_query(index: &Index, query: &Query) -> Vec<u32> {
    let mut ids: Vec<u32> = (0..index.count() as u32).collect();

    if let Some(exts) = &query.ext {
        let mut ext_ids: Vec<u32> = Vec::new();
        for ext in exts {
            if let Some(ids_for_ext) = index.by_extension(ext) {
                ext_ids.extend(ids_for_ext);
            }
        }
        ext_ids.sort_unstable();
        ext_ids.dedup();
        ids = ext_ids;
    }

    let compiled = compile_terms(&query.terms);

    ids.into_iter()
        .filter(|id| {
            let entry = &index.entries[*id as usize];
            entry_matches(entry, query, &compiled)
        })
        .collect()
}

fn entry_matches(entry: &Entry, query: &Query, compiled: &CompiledTerms) -> bool {
    if query.require_file && entry.is_dir {
        return false;
    }
    if query.require_folder && !entry.is_dir {
        return false;
    }

    if let Some(path_filter) = &query.path_filter {
        let haystack = if query.match_case {
            entry.path.as_bytes()
        } else {
            &[]
        };
        let needle_bytes: Vec<u8> = if query.match_case {
            path_filter.as_bytes().to_vec()
        } else {
            path_filter.to_lowercase().bytes().collect()
        };
        if query.match_case {
            if !haystack
                .windows(needle_bytes.len())
                .any(|w| w == needle_bytes)
            {
                return false;
            }
        } else {
            let path_lower: Vec<u8> = entry.path.to_lowercase().bytes().collect();
            if !path_lower
                .windows(needle_bytes.len())
                .any(|w| w == needle_bytes)
            {
                return false;
            }
        }
    }

    if let Some(size_filter) = &query.size {
        if !in_range(entry.size, size_filter) {
            return false;
        }
    }

    if let Some(dm) = &query.date_modified {
        if !in_range(entry.modified, dm) {
            return false;
        }
    }

    if let Some(dc) = &query.date_created {
        if !in_range(entry.created, dc) {
            return false;
        }
    }

    if let Some(da) = &query.date_accessed {
        if !in_range(entry.accessed, da) {
            return false;
        }
    }

    if let Some(attrs) = &query.attributes {
        if attrs.dir.is_some() && attrs.dir != Some(entry.is_dir) {
            return false;
        }
    }

    if compiled.items.is_empty() {
        return true;
    }

    compiled
        .items
        .iter()
        .all(|term| compiled_term_matches(entry, term, query))
}

fn compiled_term_matches(entry: &Entry, term: &CompiledTerm, query: &Query) -> bool {
    match term {
        CompiledTerm::Substring(s) => {
            let needle = if query.match_case {
                s.as_bytes().to_vec()
            } else {
                s.to_lowercase().bytes().collect()
            };
            if needle.is_empty() {
                return true;
            }
            if query.match_path {
                let haystack: Vec<u8> = if query.match_case {
                    entry.path.as_bytes().to_vec()
                } else {
                    entry.path.to_lowercase().bytes().collect()
                };
                if query.match_whole_word {
                    contains_whole_word_bytes(&haystack, &needle)
                } else {
                    haystack.windows(needle.len()).any(|w| w == needle)
                }
            } else {
                let name = entry.name();
                if query.match_whole_word {
                    let haystack: Vec<u8> = if query.match_case {
                        name.as_bytes().to_vec()
                    } else {
                        name.to_lowercase().bytes().collect()
                    };
                    contains_whole_word_bytes(&haystack, &needle)
                } else if query.match_case {
                    name.as_bytes().windows(needle.len()).any(|w| w == needle)
                } else {
                    contains_ignore_case(name, &needle)
                }
            }
        }
        CompiledTerm::Wildcard(pattern) => {
            let text = if query.match_path {
                &entry.path
            } else {
                entry.name()
            };
            let pattern = if query.match_case {
                pattern.clone()
            } else {
                pattern.to_lowercase()
            };
            let target = if query.match_case {
                text.to_string()
            } else {
                text.to_lowercase()
            };
            if query.whole_filename {
                glob_match(&target, &pattern)
            } else if query.match_whole_word {
                glob_match_word(&target, &pattern)
            } else {
                glob_match_substring(&target, &pattern)
            }
        }
        CompiledTerm::Regex(re) => {
            let text = if query.match_path {
                &entry.path
            } else {
                entry.name()
            };
            if query.match_case {
                regex_matches(re, text, query.match_whole_word)
            } else {
                regex_matches(re, &text.to_lowercase(), query.match_whole_word)
            }
        }
        CompiledTerm::Not(inner) => !compiled_term_matches(entry, inner, query),
        CompiledTerm::Or(items) => items
            .iter()
            .any(|item| compiled_term_matches(entry, item, query)),
    }
}

fn in_range<T: PartialOrd + Copy>(value: T, range: &RangeFilter<T>) -> bool {
    if let Some(min) = range.min {
        if value < min {
            return false;
        }
    }
    if let Some(max) = range.max {
        if value > max {
            return false;
        }
    }
    true
}

fn contains_whole_word_bytes(text: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if !text.is_ascii() || !needle.is_ascii() {
        let text = std::str::from_utf8(text).unwrap_or_default();
        let needle = std::str::from_utf8(needle).unwrap_or_default();
        return contains_whole_word(text, needle);
    }

    word_spans_bytes(text)
        .into_iter()
        .any(|(start, end)| &text[start..end] == needle)
}

fn regex_matches(re: &Regex, text: &str, whole_word: bool) -> bool {
    if !whole_word {
        return re.is_match(text);
    }

    word_spans(text).into_iter().any(|(start, end)| {
        let word = &text[start..end];
        re.find_iter(word)
            .any(|m| m.start() == 0 && m.end() == word.len())
    })
}

fn glob_match_word(text: &str, pattern: &str) -> bool {
    word_spans(text)
        .into_iter()
        .any(|(start, end)| glob_match(&text[start..end], pattern))
}

fn contains_whole_word(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }

    word_spans(text)
        .into_iter()
        .any(|(start, end)| &text[start..end] == needle)
}

fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;

    for (idx, ch) in text.char_indices() {
        if is_word_char(ch) {
            if start.is_none() {
                start = Some(idx);
            }
        } else if let Some(word_start) = start.take() {
            spans.push((word_start, idx));
        }
    }

    if let Some(word_start) = start {
        spans.push((word_start, text.len()));
    }

    spans
}

fn word_spans_bytes(text: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;

    for (idx, &ch) in text.iter().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == b'_' {
            if start.is_none() {
                start = Some(idx);
            }
        } else if let Some(word_start) = start.take() {
            spans.push((word_start, idx));
        }
    }

    if let Some(word_start) = start {
        spans.push((word_start, text.len()));
    }

    spans
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn glob_match(text: &str, pattern: &str) -> bool {
    let mut chars = text.chars().peekable();
    let mut pat = pattern.chars().peekable();

    while let Some(p) = pat.next() {
        match p {
            '*' => {
                while pat.peek() == Some(&'*') {
                    pat.next();
                }
                let next = pat.peek().copied();
                if next.is_none() {
                    return true;
                }
                while let Some(c) = chars.peek().copied() {
                    if Some(c) == next {
                        let text_rest: String = chars.clone().collect();
                        let pat_rest: String = pat.clone().collect();
                        if glob_match(&text_rest, &pat_rest) {
                            return true;
                        }
                    }
                    chars.next();
                }
                return false;
            }
            '?' => {
                if chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }

    chars.next().is_none()
}

fn glob_match_substring(text: &str, pattern: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text.contains(pattern);
    }
    for i in 0..text.chars().count() {
        let suffix: String = text.chars().skip(i).collect();
        if glob_match(&suffix, pattern) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests;
