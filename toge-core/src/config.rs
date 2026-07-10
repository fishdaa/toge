//! Configuration loading and representation.

use std::env;
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
    pub keyboard: KeyboardConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorOrder {
    OrAnd,
    AndOr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardConfig {
    pub new_window_hotkey: String,
    pub show_window_hotkey: String,
    pub toggle_window_hotkey: String,
    pub command_shortcuts: Vec<KeyboardShortcutConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardShortcutConfig {
    pub command_id: String,
    pub scope: KeyboardScope,
    pub accelerator: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardScope {
    Global,
    SearchEdit,
    ResultList,
}

impl Config {
    pub fn default_config() -> Self {
        Self {
            roots: default_roots(),
            exclude_fstypes: vec!["tmpfs".into(), "nfs4".into(), "fuse.sshfs".into()],
            exclude_hidden: false,
            exclude_patterns: Vec::new(),
            exclude_folders: Vec::new(),
            include_only: Vec::new(),
            index_size: true,
            index_date_modified: false,
            index_date_created: false,
            index_date_accessed: false,
            index_permissions: false,
            fast_sort_extension: false,
            fast_sort_path: false,
            whole_filename_wildcards: true,
            operator_precedence: OperatorOrder::OrAnd,
            poll_interval_secs: 300,
            keyboard: KeyboardConfig::default(),
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
        let mut keyboard_shortcuts_global = Vec::new();
        let mut keyboard_shortcuts_search_edit = Vec::new();
        let mut keyboard_shortcuts_result_list = Vec::new();

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
                "keyboard" => match key {
                    "new_window_hotkey" => {
                        cfg.keyboard.new_window_hotkey = parse_string(value)?;
                    }
                    "show_window_hotkey" => {
                        cfg.keyboard.show_window_hotkey = parse_string(value)?;
                    }
                    "toggle_window_hotkey" => {
                        cfg.keyboard.toggle_window_hotkey = parse_string(value)?;
                    }
                    _ => {}
                },
                "keyboard.shortcuts" => match key {
                    "global" => keyboard_shortcuts_global = parse_string_array(value)?,
                    "search_edit" => keyboard_shortcuts_search_edit = parse_string_array(value)?,
                    "result_list" => keyboard_shortcuts_result_list = parse_string_array(value)?,
                    _ => {}
                },
                _ => {}
            }
        }

        cfg.keyboard.command_shortcuts = parse_keyboard_shortcuts(
            keyboard_shortcuts_global,
            keyboard_shortcuts_search_edit,
            keyboard_shortcuts_result_list,
        )?;

        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        std::fs::write(path, self.to_toml()).map_err(|e| e.to_string())
    }

    pub fn to_toml(&self) -> String {
        let global = format_shortcuts(&self.keyboard.command_shortcuts, KeyboardScope::Global);
        let search_edit =
            format_shortcuts(&self.keyboard.command_shortcuts, KeyboardScope::SearchEdit);
        let result_list =
            format_shortcuts(&self.keyboard.command_shortcuts, KeyboardScope::ResultList);

        format!(
            "[index]\nsize = {size}\ndate_modified = {date_modified}\ndate_created = {date_created}\ndate_accessed = {date_accessed}\npermissions = {permissions}\nfast_extension = {fast_extension}\nwhole_filename_wildcards = {whole_filename_wildcards}\noperator_precedence = {operator_precedence}\n\n[roots]\nauto_detect = {auto_detect}\ninclude = {roots}\nexclude_fstypes = {exclude_fstypes}\n\n[exclude]\nhidden_files = {exclude_hidden}\npatterns = {exclude_patterns}\nfolders = {exclude_folders}\ninclude_only = {include_only}\n\n[polling]\ninterval_secs = {poll_interval_secs}\n\n[keyboard]\nnew_window_hotkey = {new_window_hotkey}\nshow_window_hotkey = {show_window_hotkey}\ntoggle_window_hotkey = {toggle_window_hotkey}\n\n[keyboard.shortcuts]\nglobal = {global}\nsearch_edit = {search_edit}\nresult_list = {result_list}\n",
            size = self.index_size,
            date_modified = self.index_date_modified,
            date_created = self.index_date_created,
            date_accessed = self.index_date_accessed,
            permissions = self.index_permissions,
            fast_extension = self.fast_sort_extension,
            whole_filename_wildcards = self.whole_filename_wildcards,
            operator_precedence = match self.operator_precedence {
                OperatorOrder::OrAnd => "or_and",
                OperatorOrder::AndOr => "and_or",
            },
            auto_detect = self.roots.is_empty(),
            roots = format_string_array(
                &self
                    .roots
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
            ),
            exclude_fstypes = format_string_array(&self.exclude_fstypes),
            exclude_hidden = self.exclude_hidden,
            exclude_patterns = format_string_array(&self.exclude_patterns),
            exclude_folders = format_string_array(&self.exclude_folders),
            include_only = format_string_array(&self.include_only),
            poll_interval_secs = self.poll_interval_secs,
            new_window_hotkey = format_string(&self.keyboard.new_window_hotkey),
            show_window_hotkey = format_string(&self.keyboard.show_window_hotkey),
            toggle_window_hotkey = format_string(&self.keyboard.toggle_window_hotkey),
            global = format_string_array(&global),
            search_edit = format_string_array(&search_edit),
            result_list = format_string_array(&result_list),
        )
    }
}

fn default_roots() -> Vec<PathBuf> {
    default_home_dir().into_iter().collect()
}

fn default_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            new_window_hotkey: "Ctrl+N".to_string(),
            show_window_hotkey: String::new(),
            toggle_window_hotkey: String::new(),
            command_shortcuts: Vec::new(),
        }
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

fn parse_string(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return Err(format!("expected string, got: {}", s));
    }
    Ok(s[1..s.len() - 1].replace("\\\"", "\""))
}

fn parse_keyboard_shortcuts(
    global: Vec<String>,
    search_edit: Vec<String>,
    result_list: Vec<String>,
) -> Result<Vec<KeyboardShortcutConfig>, String> {
    let mut out = Vec::new();

    for (scope, entries) in [
        (KeyboardScope::Global, global),
        (KeyboardScope::SearchEdit, search_edit),
        (KeyboardScope::ResultList, result_list),
    ] {
        for entry in entries {
            let (command_id, accelerator) = entry
                .split_once('|')
                .ok_or_else(|| format!("invalid keyboard shortcut entry: {}", entry))?;
            out.push(KeyboardShortcutConfig {
                command_id: command_id.trim().to_string(),
                scope,
                accelerator: accelerator.trim().to_string(),
            });
        }
    }

    Ok(out)
}

fn format_string(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn format_string_array(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| format_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{}]", items)
}

fn format_shortcuts(shortcuts: &[KeyboardShortcutConfig], scope: KeyboardScope) -> Vec<String> {
    shortcuts
        .iter()
        .filter(|shortcut| shortcut.scope == scope)
        .map(|shortcut| format!("{}|{}", shortcut.command_id, shortcut.accelerator))
        .collect()
}

#[cfg(test)]
mod tests;
