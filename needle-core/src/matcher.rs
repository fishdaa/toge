//! Evaluate a parsed Query against Index entries.

use crate::index::{Entry, Index};
use crate::query::Query;

pub fn match_query(index: &Index, query: &Query) -> Vec<u32> {
    let _ = (index, query);
    todo!()
}

pub fn entry_matches(entry: &Entry, query: &Query) -> bool {
    let _ = (entry, query);
    todo!()
}

#[cfg(test)]
mod tests;
