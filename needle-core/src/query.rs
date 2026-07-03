//! Everything-compatible query parser.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchMode {
    Substring,
    Wildcard,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub raw: String,
    pub mode: SearchMode,
    pub match_case: bool,
    pub match_whole_word: bool,
    pub match_path: bool,
    pub require_file: bool,
    pub require_folder: bool,
    pub whole_filename: bool,
    pub terms: Vec<TextTerm>,
    pub ext: Option<Vec<String>>,
    pub path_filter: Option<String>,
    pub size: Option<RangeFilter<u64>>,
    pub date_modified: Option<RangeFilter<i64>>,
    pub date_created: Option<RangeFilter<i64>>,
    pub date_accessed: Option<RangeFilter<i64>>,
    pub attributes: Option<AttributeFilter>,
    pub offset: usize,
    pub max_results: usize,
    pub sort: Sort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextTerm {
    Substring(String),
    Wildcard(String),
    Regex(String),
    Not(Box<TextTerm>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeFilter<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeFilter {
    pub dir: Option<bool>,
    pub hidden: Option<bool>,
    pub readonly: Option<bool>,
    pub system: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    NameAsc,
    NameDesc,
    PathAsc,
    PathDesc,
    SizeDesc,
    ModifiedDesc,
    CreatedDesc,
    AccessedDesc,
    ExtensionAsc,
}

impl Query {
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let _ = input;
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests;
