//! Configuration loading and representation.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    pub exclude_fstypes: Vec<String>,
    pub exclude_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub exclude_folders: Vec<String>,
    pub include_only: Vec<String>,
    pub index_size: bool,
    pub index_date_modified: bool,
    pub index_date_created: bool,
    pub index_date_accessed: bool,
    pub index_permissions: bool,
    pub fast_sort_extension: bool,
    pub fast_sort_path: bool,
    pub whole_filename_wildcards: bool,
    pub operator_precedence: OperatorOrder,
    pub poll_interval_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorOrder {
    OrAnd,
    AndOr,
}

impl Config {
    pub fn default_config() -> Self {
        Self {
            roots: Vec::new(),
            exclude_fstypes: vec!["tmpfs".into(), "nfs4".into(), "fuse.sshfs".into()],
            exclude_hidden: false,
            exclude_patterns: Vec::new(),
            exclude_folders: Vec::new(),
            include_only: Vec::new(),
            index_size: false,
            index_date_modified: false,
            index_date_created: false,
            index_date_accessed: false,
            index_permissions: false,
            fast_sort_extension: false,
            fast_sort_path: false,
            whole_filename_wildcards: true,
            operator_precedence: OperatorOrder::OrAnd,
            poll_interval_secs: 300,
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default_config());
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut cfg = Self::default_config();
        let mut section = String::new();

        for (line_no, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].to_string();
                continue;
            }

            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("expected key=value at line {}", line_no + 1))?;
            let key = key.trim();
            let value = value.trim();

            match section.as_str() {
                "index" => match key {
                    "size" => cfg.index_size = parse_bool(value)?,
                    "date_modified" => cfg.index_date_modified = parse_bool(value)?,
                    "date_created" => cfg.index_date_created = parse_bool(value)?,
                    "date_accessed" => cfg.index_date_accessed = parse_bool(value)?,
                    "permissions" => cfg.index_permissions = parse_bool(value)?,
                    "fast_extension" => cfg.fast_sort_extension = parse_bool(value)?,
                    "whole_filename_wildcards" => cfg.whole_filename_wildcards = parse_bool(value)?,
                    "operator_precedence" => {
                        cfg.operator_precedence = match value {
                            "or_and" => OperatorOrder::OrAnd,
                            "and_or" => OperatorOrder::AndOr,
                            _ => return Err(format!("unknown precedence: {}", value)),
                        }
                    }
                    _ => {}
                },
                "roots" => match key {
                    "auto_detect" => {
                        if !parse_bool(value)? {
                            cfg.roots.clear();
                        }
                    }
                    "include" => {
                        cfg.roots = parse_string_array(value)?
                            .into_iter()
                            .map(PathBuf::from)
                            .collect()
                    }
                    "exclude_fstypes" => cfg.exclude_fstypes = parse_string_array(value)?,
                    _ => {}
                },
                "exclude" => match key {
                    "hidden_files" => cfg.exclude_hidden = parse_bool(value)?,
                    "patterns" => cfg.exclude_patterns = parse_string_array(value)?,
                    "folders" => cfg.exclude_folders = parse_string_array(value)?,
                    "include_only" => cfg.include_only = parse_string_array(value)?,
                    _ => {}
                },
                "polling" if key == "interval_secs" => {
                    cfg.poll_interval_secs = value.parse().map_err(|_| "invalid interval")?;
                }
                "polling" => {}
                _ => {}
            }
        }

        Ok(cfg)
    }
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("expected true/false, got: {}", s)),
    }
}

fn parse_string_array(s: &str) -> Result<Vec<String>, String> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Err(format!("expected array, got: {}", s));
    }
    let inner = &s[1..s.len() - 1];
    let mut out = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let item = item.trim_matches('"').to_string();
        out.push(item);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
