//! CLI option parsing (mirrors ES syntax).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdlOptions {
    pub search: String,
    pub regex: Option<String>,
    pub case: bool,
    pub whole_word: bool,
    pub match_path: bool,
    pub diacritics: bool,
    pub offset: usize,
    pub max_results: usize,
    pub path_filter: Option<String>,
    pub show_size: bool,
    pub show_modified: bool,
    pub show_created: bool,
    pub show_extension: bool,
    pub sort: Option<String>,
    pub sort_ascending: bool,
    pub format: OutputFormat,
    pub export_file: Option<String>,
    pub pause: bool,
    pub no_header: bool,
    pub highlight: bool,
    pub highlight_color: u8,
    pub status: bool,
    pub save_db: bool,
    pub reindex: bool,
    pub get_result_count: bool,
    pub get_total_size: bool,
    pub no_result_error: bool,
    pub hide_empty: bool,
    pub help: bool,
    pub version: bool,
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Default,
    Csv,
    Tsv,
    Txt,
    Efu,
}

impl Default for NdlOptions {
    fn default() -> Self {
        Self {
            search: String::new(),
            regex: None,
            case: false,
            whole_word: false,
            match_path: false,
            diacritics: false,
            offset: 0,
            max_results: usize::MAX,
            path_filter: None,
            show_size: false,
            show_modified: false,
            show_created: false,
            show_extension: false,
            sort: None,
            sort_ascending: false,
            format: OutputFormat::Default,
            export_file: None,
            pause: false,
            no_header: false,
            highlight: false,
            highlight_color: 2,
            status: false,
            save_db: false,
            reindex: false,
            get_result_count: false,
            get_total_size: false,
            no_result_error: false,
            hide_empty: false,
            help: false,
            version: false,
            config_path: None,
        }
    }
}

impl NdlOptions {
    pub fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut opts = Self::default();
        let mut positional: Vec<String> = Vec::new();
        let mut iter = args.into_iter().peekable();

        // Skip program name.
        iter.next();

        while let Some(arg) = iter.next() {
            if arg.starts_with('/') {
                parse_windows_flag(&arg, &mut positional)?;
                continue;
            }

            if !arg.starts_with('-') {
                positional.push(arg);
                continue;
            }

            let flag = arg.trim_start_matches('-');
            match flag {
                "r" | "regex" => {
                    let value = iter.next().ok_or("missing regex value")?;
                    opts.regex = Some(value.clone());
                    positional.push(format!("regex:{}", value));
                }
                "i" | "case" => opts.case = true,
                "w" | "ww" | "whole-word" => opts.whole_word = true,
                "p" | "match-path" => opts.match_path = true,
                "a" | "diacritics" => opts.diacritics = true,
                "o" | "offset" => {
                    let value = iter.next().ok_or("missing offset value")?;
                    opts.offset = parse_usize(&value)?;
                }
                "n" | "max-results" => {
                    let value = iter.next().ok_or("missing max-results value")?;
                    opts.max_results = parse_usize(&value)?;
                }
                "path" => {
                    let value = iter.next().ok_or("missing path value")?;
                    opts.path_filter = Some(value.clone());
                    positional.push(format!("path:{}", value));
                }
                "size" => opts.show_size = true,
                "dm" | "date-modified" => opts.show_modified = true,
                "dc" | "date-created" => opts.show_created = true,
                "ext" | "extension" => opts.show_extension = true,
                "s" => opts.sort = Some("path".into()),
                "sort" => {
                    let value = iter.next().ok_or("missing sort value")?;
                    opts.sort = Some(value);
                }
                "sort-ascending" => opts.sort_ascending = true,
                "sort-descending" => opts.sort_ascending = false,
                "csv" => opts.format = OutputFormat::Csv,
                "tsv" => opts.format = OutputFormat::Tsv,
                "txt" => opts.format = OutputFormat::Txt,
                "efu" => opts.format = OutputFormat::Efu,
                "export-csv" => {
                    let value = iter.next().ok_or("missing export file")?;
                    opts.export_file = Some(value);
                    opts.format = OutputFormat::Csv;
                }
                "export-tsv" => {
                    let value = iter.next().ok_or("missing export file")?;
                    opts.export_file = Some(value);
                    opts.format = OutputFormat::Tsv;
                }
                "export-txt" => {
                    let value = iter.next().ok_or("missing export file")?;
                    opts.export_file = Some(value);
                    opts.format = OutputFormat::Txt;
                }
                "export-efu" => {
                    let value = iter.next().ok_or("missing export file")?;
                    opts.export_file = Some(value);
                    opts.format = OutputFormat::Efu;
                }
                "pause" | "more" => opts.pause = true,
                "no-header" => opts.no_header = true,
                "highlight" => opts.highlight = true,
                "highlight-color" => {
                    let value = iter.next().ok_or("missing highlight color")?;
                    opts.highlight_color = value.parse().map_err(|_| "invalid highlight color")?;
                }
                "status" => opts.status = true,
                "save-db" => opts.save_db = true,
                "reindex" => opts.reindex = true,
                "get-result-count" => opts.get_result_count = true,
                "get-total-size" => opts.get_total_size = true,
                "no-result-error" => opts.no_result_error = true,
                "hide-empty-search-results" => opts.hide_empty = true,
                "h" | "help" => opts.help = true,
                "v" | "version" => opts.version = true,
                "config" => {
                    let value = iter.next().ok_or("missing config path")?;
                    opts.config_path = Some(value);
                }
                _ => return Err(format!("unknown flag: {}", arg)),
            }
        }

        let mut search_parts = Vec::new();
        if opts.case {
            search_parts.push("case:".to_string());
        }
        if opts.whole_word {
            search_parts.push("ww:".to_string());
        }
        if opts.match_path {
            search_parts.push("path:".to_string());
        }
        if let Some(sort) = &opts.sort {
            let sort_lower = sort.to_lowercase();
            let has_direction = sort_lower.ends_with("-asc") || sort_lower.ends_with("-desc");
            let sort_str = if has_direction {
                sort.clone()
            } else if opts.sort_ascending {
                format!("{}-asc", sort)
            } else {
                format!("{}-desc", sort)
            };
            search_parts.push(format!("sort:{}", sort_str));
        }
        search_parts.extend(positional);

        opts.search = search_parts.join(" ");
        Ok(opts)
    }
}

fn parse_usize(s: &str) -> Result<usize, String> {
    s.parse::<usize>()
        .map_err(|_| format!("invalid number: {}", s))
}

fn parse_windows_flag(arg: &str, positional: &mut Vec<String>) -> Result<(), String> {
    if arg == "/ad" {
        positional.insert(0, "folder:".to_string());
        return Ok(());
    }
    if arg == "/a-d" {
        positional.insert(0, "file:".to_string());
        return Ok(());
    }
    if let Some(attrs) = arg.strip_prefix("/a") {
        for c in attrs.chars() {
            if c == 'D' {
                positional.insert(0, "folder:".to_string());
            }
        }
        return Ok(());
    }
    if let Some(sort) = arg.strip_prefix("/o") {
        match sort {
            "N" => positional.push("sort:name-asc".to_string()),
            "-N" => positional.push("sort:name-desc".to_string()),
            "S" => positional.push("sort:size-desc".to_string()),
            "-S" => positional.push("sort:size-asc".to_string()),
            "E" => positional.push("sort:extension-asc".to_string()),
            "-E" => positional.push("sort:extension-desc".to_string()),
            "D" => positional.push("sort:date-modified-desc".to_string()),
            "-D" => positional.push("sort:date-modified-asc".to_string()),
            _ => return Err(format!("unknown sort flag: /o{}", sort)),
        }
        return Ok(());
    }
    Err(format!("unknown windows flag: {}", arg))
}

#[cfg(test)]
mod tests;
