//! Evaluate a parsed Query against Index entries.

use crate::index::{Entry, Index, contains_ignore_case};
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

// Case-insensitive needles are lowercased once here rather than per entry.
fn compile_terms(terms: &[TextTerm], match_case: bool) -> CompiledTerms {
    let cased = |text: &String| {
        if match_case {
            text.clone()
        } else {
            text.to_lowercase()
        }
    };
    let items = terms
        .iter()
        .map(|term| match term {
            TextTerm::Substring(s) => CompiledTerm::Substring(cased(s)),
            TextTerm::Wildcard(p) => CompiledTerm::Wildcard(cased(p)),
            TextTerm::Regex(p) => CompiledTerm::Regex(
                Regex::new(p).expect("regex patterns should be validated during query parsing"),
            ),
            TextTerm::Not(inner) => CompiledTerm::Not(Box::new(
                compile_terms(&[inner.as_ref().clone()], match_case)
                    .items
                    .into_iter()
                    .next()
                    .unwrap(),
            )),
            TextTerm::Or(items) => CompiledTerm::Or(compile_terms(items, match_case).items),
        })
        .collect();
    CompiledTerms { items }
}

/// A reusable matcher for incremental scans, without a result-ID buffer.
/// Sorting and result limits are the caller's responsibility.
pub struct QueryMatcher {
    query: Query,
    compiled: CompiledTerms,
}

impl QueryMatcher {
    pub fn new(query: Query) -> Self {
        let compiled = compile_terms(&query.terms, query.match_case);
        Self { query, compiled }
    }

    pub fn matches(&self, entry: &Entry) -> bool {
        if let Some(exts) = &self.query.ext
            && (entry.is_dir || !exts.iter().any(|ext| ext == entry.extension()))
        {
            return false;
        }
        entry_matches(entry, &self.query, &self.compiled)
    }
}

/// Lazily yield matching IDs in index order, using memory independent of index size.
/// This scans entries rather than materializing trigram candidates. Dropping the
/// iterator cancels the scan. Metadata is read as stored in the index.
pub fn iter_query<'a>(index: &'a Index, query: &Query) -> impl Iterator<Item = u32> + 'a {
    let matcher = QueryMatcher::new(query.clone());
    index
        .entries
        .iter()
        .enumerate()
        .filter(move |(_, entry)| matcher.matches(entry))
        .map(|(id, _)| id as u32)
}

/// Sorted (index-order) IDs that may match, taken from the extension and
/// trigram indexes, or `None` when the query has no selective seed and every
/// entry must be scanned. Candidates still need the full matcher.
pub fn candidate_ids(index: &Index, query: &Query) -> Option<Vec<u32>> {
    let seed = if query.match_path {
        None
    } else {
        query
            .terms
            .iter()
            .filter_map(|term| match term {
                TextTerm::Substring(value) if value.len() >= 3 => Some(value.as_str()),
                _ => None,
            })
            .max_by_key(|value| value.len())
    };
    if let Some(exts) = &query.ext {
        let mut ext_ids: Vec<u32> = Vec::new();
        for ext in exts {
            if let Some(ids_for_ext) = index.by_extension(ext) {
                ext_ids.extend(ids_for_ext);
            }
        }
        ext_ids.sort_unstable();
        ext_ids.dedup();
        Some(if let Some(seed) = seed {
            intersect_sorted_ids(&ext_ids, &index.search_substring(seed))
        } else {
            ext_ids
        })
    } else {
        seed.map(|seed| index.search_substring(seed))
    }
}

pub fn match_query(index: &Index, query: &Query) -> Vec<u32> {
    // Seed directly from the trigram index so a selective filename query does
    // not first allocate an ID vector for every entry in the filesystem.
    // The full matcher below still enforces every query option.
    let mut ids =
        candidate_ids(index, query).unwrap_or_else(|| (0..index.count() as u32).collect());
    let compiled = compile_terms(&query.terms, query.match_case);

    ids.retain(|&id| {
        let entry = &index.entries[id as usize];
        entry_matches(entry, query, &compiled)
    });
    ids
}

fn intersect_sorted_ids(left: &[u32], right: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(left.len().min(right.len()));
    let (mut left_idx, mut right_idx) = (0, 0);

    while left_idx < left.len() && right_idx < right.len() {
        match left[left_idx].cmp(&right[right_idx]) {
            std::cmp::Ordering::Less => left_idx += 1,
            std::cmp::Ordering::Greater => right_idx += 1,
            std::cmp::Ordering::Equal => {
                result.push(left[left_idx]);
                left_idx += 1;
                right_idx += 1;
            }
        }
    }

    result
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

    if let Some(size_filter) = &query.size
        && !in_range(entry.size, size_filter)
    {
        return false;
    }

    if let Some(dm) = &query.date_modified
        && !in_range(entry.modified, dm)
    {
        return false;
    }

    if let Some(dc) = &query.date_created
        && !in_range(entry.created, dc)
    {
        return false;
    }

    if let Some(da) = &query.date_accessed
        && !in_range(entry.accessed, da)
    {
        return false;
    }

    if let Some(attrs) = &query.attributes
        && attrs.dir.is_some()
        && attrs.dir != Some(entry.is_dir)
    {
        return false;
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
            let needle = s.as_bytes();
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
                    contains_whole_word_bytes(&haystack, needle)
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
                    contains_whole_word_bytes(&haystack, needle)
                } else if query.match_case {
                    name.as_bytes().windows(needle.len()).any(|w| w == needle)
                } else {
                    contains_ignore_case(name, needle)
                }
            }
        }
        CompiledTerm::Wildcard(pattern) => {
            let text = if query.match_path {
                &entry.path
            } else {
                entry.name()
            };
            // The pattern is already lowercased. ASCII text is folded while
            // matching instead of copying every entry's name to lowercase it.
            let lowered;
            let (target, fold) = if query.match_case {
                (text, false)
            } else if text.is_ascii() {
                (text, true)
            } else {
                lowered = text.to_lowercase();
                (lowered.as_str(), false)
            };
            if query.whole_filename {
                glob_match_from(target.as_bytes(), pattern.as_bytes(), false, fold)
            } else if query.match_whole_word {
                glob_match_word(target, pattern, fold)
            } else {
                glob_match_substring(target, pattern, fold)
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
    if let Some(min) = range.min
        && value < min
    {
        return false;
    }
    if let Some(max) = range.max
        && value > max
    {
        return false;
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

fn glob_match_word(text: &str, pattern: &str, fold: bool) -> bool {
    word_spans(text).into_iter().any(|(start, end)| {
        glob_match_from(
            &text.as_bytes()[start..end],
            pattern.as_bytes(),
            false,
            fold,
        )
    })
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

#[cfg(test)]
fn glob_match(text: &str, pattern: &str) -> bool {
    glob_match_from(text.as_bytes(), pattern.as_bytes(), false, false)
}

/// Whether `pattern` matches some suffix of `text`, i.e. `*` + `pattern`.
fn glob_match_substring(text: &str, pattern: &str, fold: bool) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return if fold {
            contains_ignore_case(text, pattern.as_bytes())
        } else {
            text.contains(pattern)
        };
    }
    glob_match_from(text.as_bytes(), pattern.as_bytes(), true, fold)
}

/// Iterative `*`/`?` matching with single-star backtracking: O(text × pattern)
/// at worst and allocation-free, since it runs once per indexed entry. Works on
/// UTF-8 bytes; `?` and backtracking step whole characters, so literal bytes are
/// only ever compared from a character boundary. With `fold`, text bytes are
/// ASCII-lowercased before comparing with the (lowercase) pattern.
fn glob_match_from(text: &[u8], pattern: &[u8], leading_star: bool, fold: bool) -> bool {
    let char_len = |lead: u8| match lead {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    };
    let (mut ti, mut pi) = (0, 0);
    // Pattern index after the last `*`, and the text index it is retried from.
    let mut star = leading_star.then_some((0, 0));
    while ti < text.len() {
        match pattern.get(pi) {
            Some(b'*') => {
                pi += 1;
                star = Some((pi, ti));
            }
            Some(b'?') => {
                ti += char_len(text[ti]);
                pi += 1;
            }
            Some(&byte) if byte == text[ti] || (fold && byte == text[ti].to_ascii_lowercase()) => {
                ti += 1;
                pi += 1;
            }
            _ => {
                let Some((star_pi, star_ti)) = star else {
                    return false;
                };
                let retry = star_ti + char_len(text[star_ti]);
                star = Some((star_pi, retry));
                (pi, ti) = (star_pi, retry);
            }
        }
    }
    pattern[pi..].iter().all(|&byte| byte == b'*')
}

#[cfg(test)]
mod tests;
