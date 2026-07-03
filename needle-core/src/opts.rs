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

impl NdlOptions {
    pub fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let _ = args;
        todo!()
    }
}

#[cfg(test)]
mod tests;
