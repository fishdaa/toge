//! Evaluate a parsed Query against Index entries.

use crate::index::{Entry, Index};
use crate::query::{Query, RangeFilter, TextTerm};

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

    ids.into_iter()
        .filter(|id| {
            let entry = &index.entries[*id as usize];
            entry_matches(entry, query)
        })
        .collect()
}

pub fn entry_matches(entry: &Entry, query: &Query) -> bool {
    if query.require_file && entry.is_dir {
        return false;
    }
    if query.require_folder && !entry.is_dir {
        return false;
    }

    if let Some(path_filter) = &query.path_filter {
        let haystack = if query.match_case {
            entry.path.clone()
        } else {
            entry.path.to_lowercase()
        };
        let needle = if query.match_case {
            path_filter.clone()
        } else {
            path_filter.to_lowercase()
        };
        if !haystack.contains(&needle) {
            return false;
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

    if query.terms.is_empty() {
        return true;
    }

    query.terms.iter().all(|term| term_matches(entry, term, query))
}

fn term_matches(entry: &Entry, term: &TextTerm, query: &Query) -> bool {
    match term {
        TextTerm::Substring(s) => {
            let haystack = haystack(entry, query);
            let needle = normalize(s, query);
            haystack.contains(&needle)
        }
        TextTerm::Wildcard(pattern) => {
            let name = entry.name();
            let pattern = if query.match_case {
                pattern.clone()
            } else {
                pattern.to_lowercase()
            };
            let target = if query.match_case {
                name.to_string()
            } else {
                name.to_lowercase()
            };
            if query.whole_filename {
                glob_match(&target, &pattern)
            } else {
                glob_match_substring(&target, &pattern)
            }
        }
        TextTerm::Regex(pattern) => {
            let haystack = haystack(entry, query);
            haystack.contains(&normalize(pattern, query))
        }
        TextTerm::Not(inner) => !term_matches(entry, inner, query),
    }
}

fn haystack(entry: &Entry, query: &Query) -> String {
    let text = if query.match_path {
        &entry.path
    } else {
        entry.name()
    };
    if query.match_case {
        text.to_string()
    } else {
        text.to_lowercase()
    }
}

fn normalize(s: &str, query: &Query) -> String {
    if query.match_case {
        s.to_string()
    } else {
        s.to_lowercase()
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
