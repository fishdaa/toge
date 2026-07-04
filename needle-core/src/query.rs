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
    Or(Vec<TextTerm>),
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
    SizeAsc,
    SizeDesc,
    ModifiedAsc,
    ModifiedDesc,
    CreatedAsc,
    CreatedDesc,
    AccessedAsc,
    AccessedDesc,
    ExtensionAsc,
    ExtensionDesc,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            raw: String::new(),
            mode: SearchMode::Substring,
            match_case: false,
            match_whole_word: false,
            match_path: false,
            require_file: false,
            require_folder: false,
            whole_filename: false,
            terms: Vec::new(),
            ext: None,
            path_filter: None,
            size: None,
            date_modified: None,
            date_created: None,
            date_accessed: None,
            attributes: None,
            offset: 0,
            max_results: usize::MAX,
            sort: Sort::NameAsc,
        }
    }
}

impl Query {
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let mut query = Query {
            raw: input.to_string(),
            ..Self::default()
        };

        let tokens = tokenize(input)?;
        for token in tokens {
            match token {
                Token::Modifier(name, value) => apply_modifier(&mut query, &name, &value)?,
                Token::Function(name, value) => apply_function(&mut query, &name, &value)?,
                Token::Macro(name) => apply_macro(&mut query, &name)?,
                Token::Text(text) => add_text_term(&mut query, &text)?,
            }
        }

        Ok(query)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Text(String),
    Modifier(String, String),
    Function(String, String),
    Macro(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if !current.is_empty() {
                    tokens.push(classify_token(&current)?);
                    current.clear();
                }
            }
            '"' => {
                if !current.is_empty() {
                    tokens.push(classify_token(&current)?);
                    current.clear();
                }
                let mut quoted = String::new();
                for qc in chars.by_ref() {
                    if qc == '"' {
                        break;
                    }
                    quoted.push(qc);
                }
                tokens.push(Token::Text(quoted));
            }
            _ => current.push(c),
        }
    }

    if !current.is_empty() {
        tokens.push(classify_token(&current)?);
    }

    Ok(tokens)
}

fn classify_token(s: &str) -> Result<Token, ParseError> {
    if let Some((name, value)) = s.split_once(':') {
        if name.is_empty() {
            return Ok(Token::Text(s.to_string()));
        }
        let name_lower = name.to_lowercase();
        if is_modifier(&name_lower) {
            return Ok(Token::Modifier(name_lower, value.to_string()));
        }
        if is_function(&name_lower) {
            return Ok(Token::Function(name_lower, value.to_string()));
        }
        if is_file_type_macro(&name_lower) {
            return Ok(Token::Macro(name_lower));
        }
    }
    Ok(Token::Text(s.to_string()))
}

fn is_modifier(name: &str) -> bool {
    matches!(
        name,
        "case"
            | "nocase"
            | "file"
            | "folder"
            | "path"
            | "nopath"
            | "ww"
            | "noww"
            | "wildcards"
            | "nowildcards"
            | "regex"
            | "noregex"
            | "diacritics"
            | "wholefilename"
            | "nowholefilename"
    )
}

fn is_function(name: &str) -> bool {
    matches!(
        name,
        "ext"
            | "parent"
            | "size"
            | "dm"
            | "dc"
            | "da"
            | "attrib"
            | "child"
            | "depth"
            | "empty"
            | "sort"
    )
}

fn is_file_type_macro(name: &str) -> bool {
    matches!(name, "audio" | "doc" | "exe" | "pic" | "video" | "zip")
}

fn apply_modifier(query: &mut Query, name: &str, value: &str) -> Result<(), ParseError> {
    match name {
        "case" => query.match_case = true,
        "nocase" => query.match_case = false,
        "file" => query.require_file = true,
        "folder" => query.require_folder = true,
        "path" => {
            query.match_path = true;
            if !value.is_empty() {
                query.path_filter = Some(value.to_string());
            }
        }
        "nopath" => query.match_path = false,
        "ww" => query.match_whole_word = true,
        "noww" => query.match_whole_word = false,
        "wildcards" => query.mode = SearchMode::Wildcard,
        "nowildcards" => query.mode = SearchMode::Substring,
        "regex" => {
            query.mode = SearchMode::Regex;
            if !value.is_empty() {
                validate_regex(value)?;
                query.terms.push(TextTerm::Regex(value.to_string()));
            }
        }
        "noregex" => query.mode = SearchMode::Substring,
        "wholefilename" => query.whole_filename = true,
        "nowholefilename" => query.whole_filename = false,
        _ => {}
    }
    Ok(())
}

fn apply_function(query: &mut Query, name: &str, value: &str) -> Result<(), ParseError> {
    match name {
        "ext" => {
            let exts: Vec<String> = value
                .split(';')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if !exts.is_empty() {
                query.ext = Some(exts);
            }
        }
        "size" => query.size = Some(parse_size(value)?),
        "dm" => query.date_modified = Some(parse_date(value)?),
        "dc" => query.date_created = Some(parse_date(value)?),
        "da" => query.date_accessed = Some(parse_date(value)?),
        "attrib" => query.attributes = Some(parse_attributes(value)),
        "sort" => query.sort = parse_sort(value)?,
        _ => {}
    }
    Ok(())
}

fn parse_sort(value: &str) -> Result<Sort, ParseError> {
    match value.trim().to_lowercase().as_str() {
        "name" | "name-asc" => Ok(Sort::NameAsc),
        "name-desc" => Ok(Sort::NameDesc),
        "path" | "path-asc" => Ok(Sort::PathAsc),
        "path-desc" => Ok(Sort::PathDesc),
        "size" | "size-asc" => Ok(Sort::SizeAsc),
        "size-desc" => Ok(Sort::SizeDesc),
        "modified" | "date-modified" | "modified-asc" | "date-modified-asc" => {
            Ok(Sort::ModifiedAsc)
        }
        "modified-desc" | "date-modified-desc" => Ok(Sort::ModifiedDesc),
        "created" | "date-created" | "created-asc" | "date-created-asc" => Ok(Sort::CreatedAsc),
        "created-desc" | "date-created-desc" => Ok(Sort::CreatedDesc),
        "accessed" | "date-accessed" | "accessed-asc" | "date-accessed-asc" => {
            Ok(Sort::AccessedAsc)
        }
        "accessed-desc" | "date-accessed-desc" => Ok(Sort::AccessedDesc),
        "extension" | "ext" | "extension-asc" | "ext-asc" => Ok(Sort::ExtensionAsc),
        "extension-desc" | "ext-desc" => Ok(Sort::ExtensionDesc),
        other => Err(ParseError(format!("unknown sort: {}", other))),
    }
}

fn apply_macro(query: &mut Query, name: &str) -> Result<(), ParseError> {
    let exts = match name {
        "audio" => "aac;ac3;aiff;flac;m4a;mid;midi;mp3;ogg;ra;wav;wma",
        "doc" => "doc;docx;xls;xlsx;ppt;pptx;pdf;txt;rtf;csv",
        "exe" => "exe;com;bat;cmd;msi;scr;pif",
        "pic" => "bmp;gif;ico;jpg;jpeg;png;psd;svg;tif;tiff;webm",
        "video" => "avi;flv;m4v;mkv;mov;mp4;mpeg;mpg;wmv",
        "zip" => "7z;cab;bz2;gz;rar;tar;tgz;zip",
        _ => return Ok(()),
    };
    let list: Vec<String> = exts.split(';').map(|s| s.to_string()).collect();
    query.ext = Some(list);
    Ok(())
}

fn add_text_term(query: &mut Query, text: &str) -> Result<(), ParseError> {
    let mut terms = Vec::new();
    for part in text.split('|') {
        if part.is_empty() {
            continue;
        }
        let term = match query.mode {
            SearchMode::Regex => {
                validate_regex(part)?;
                TextTerm::Regex(part.to_string())
            }
            _ if part.contains('*') || part.contains('?') => {
                query.mode = SearchMode::Wildcard;
                TextTerm::Wildcard(part.to_string())
            }
            _ => TextTerm::Substring(part.to_string()),
        };
        terms.push(term);
    }

    match terms.len() {
        0 => {}
        1 => query.terms.push(terms.remove(0)),
        _ => query.terms.push(TextTerm::Or(terms)),
    }
    Ok(())
}

fn parse_size(value: &str) -> Result<RangeFilter<u64>, ParseError> {
    let value = value.trim();
    if let Some((min_s, max_s)) = value.split_once("..") {
        return Ok(RangeFilter {
            min: Some(parse_size_value(min_s.trim())?),
            max: Some(parse_size_value(max_s.trim())?),
        });
    }
    if let Some((min_s, max_s)) = value.split_once('-') {
        return Ok(RangeFilter {
            min: Some(parse_size_value(min_s.trim())?),
            max: Some(parse_size_value(max_s.trim())?),
        });
    }
    if let Some(rest) = value.strip_prefix(">=") {
        return Ok(RangeFilter {
            min: Some(parse_size_value(rest)?),
            max: None,
        });
    }
    if let Some(rest) = value.strip_prefix("<=") {
        return Ok(RangeFilter {
            min: None,
            max: Some(parse_size_value(rest)?),
        });
    }
    if let Some(rest) = value.strip_prefix('>') {
        return Ok(RangeFilter {
            min: Some(parse_size_value(rest)? + 1),
            max: None,
        });
    }
    if let Some(rest) = value.strip_prefix('<') {
        let max = parse_size_value(rest)?.checked_sub(1).ok_or_else(|| {
            ParseError("strictly less than zero bytes is not a valid size filter".into())
        })?;
        return Ok(RangeFilter {
            min: None,
            max: Some(max),
        });
    }
    Ok(RangeFilter {
        min: Some(parse_size_value(value)?),
        max: Some(parse_size_value(value)?),
    })
}

fn parse_size_value(s: &str) -> Result<u64, ParseError> {
    let s = s.trim().to_lowercase();
    if s == "empty" {
        return Ok(0);
    }
    if s == "tiny" {
        return Ok(10 * 1024);
    }
    if s == "small" {
        return Ok(100 * 1024);
    }
    if s == "medium" {
        return Ok(1024 * 1024);
    }
    if s == "large" {
        return Ok(16 * 1024 * 1024);
    }
    if s == "huge" {
        return Ok(128 * 1024 * 1024);
    }
    if s == "gigantic" {
        return Ok(128 * 1024 * 1024 + 1);
    }

    let multiplier = if let Some(num) = s.strip_suffix("gb") {
        (num.trim(), 1_000_000_000u64)
    } else if let Some(num) = s.strip_suffix("mb") {
        (num.trim(), 1_000_000u64)
    } else if let Some(num) = s.strip_suffix("kb") {
        (num.trim(), 1_000u64)
    } else {
        (s.as_str(), 1u64)
    };

    multiplier
        .0
        .parse::<u64>()
        .map(|n| n * multiplier.1)
        .map_err(|_| ParseError(format!("invalid size: {}", s)))
}

fn parse_date(value: &str) -> Result<RangeFilter<i64>, ParseError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let (start, end) = match value.trim().to_lowercase().as_str() {
        "today" => (start_of_day(now), end_of_day(now)),
        "yesterday" => (start_of_day(now - 86400), end_of_day(now - 86400)),
        _ => {
            return Ok(RangeFilter {
                min: Some(now - 86400),
                max: Some(now),
            });
        }
    };
    Ok(RangeFilter {
        min: Some(start),
        max: Some(end),
    })
}

fn start_of_day(ts: i64) -> i64 {
    (ts / 86400) * 86400
}

fn end_of_day(ts: i64) -> i64 {
    start_of_day(ts) + 86400 - 1
}

fn parse_attributes(value: &str) -> AttributeFilter {
    let mut filter = AttributeFilter {
        dir: None,
        hidden: None,
        readonly: None,
        system: None,
    };
    for c in value.to_uppercase().chars() {
        match c {
            'D' => filter.dir = Some(true),
            'H' => filter.hidden = Some(true),
            'R' => filter.readonly = Some(true),
            'S' => filter.system = Some(true),
            _ => {}
        }
    }
    filter
}

fn validate_regex(pattern: &str) -> Result<(), ParseError> {
    let mut stack = Vec::new();
    let mut escaped = false;
    for c in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '(' | '[' | '{' => stack.push(c),
            ')' => {
                if stack.pop() != Some('(') {
                    return Err(ParseError("unmatched )".into()));
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return Err(ParseError("unmatched ]".into()));
                }
            }
            '}' if stack.pop() != Some('{') => return Err(ParseError("unmatched }".into())),
            '}' => {}
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(ParseError("unclosed group".into()));
    }
    Ok(())
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
