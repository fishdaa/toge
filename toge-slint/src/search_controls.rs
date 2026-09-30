//! Search controls edit the same raw query that is sent to the daemon.
use slint::ComponentHandle;
use toge_core::query::{Query, SearchMode};

const PRESETS: [&str; 8] = [
    "", "file:", "folder:", "audio:", "doc:", "pic:", "video:", "zip:",
];

/// Keep quoted values and quoted literal modifiers together while editing.
fn tokens(raw: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (offset, ch) in raw.char_indices() {
        if ch.is_whitespace() && !quoted {
            if let Some(start) = start.take() {
                result.push(&raw[start..offset]);
            }
        } else {
            start.get_or_insert(offset);
            if ch == '"' {
                quoted = !quoted;
            }
        }
    }
    if let Some(start) = start {
        result.push(&raw[start..]);
    }
    result
}

fn modifier(token: &str) -> Option<(String, &str)> {
    if token.starts_with('"') {
        return None;
    }
    token
        .split_once(':')
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
}

pub fn set_option(raw: &str, option: &str, enabled: bool) -> String {
    let (positive, negative) = match option {
        "case" => ("case", "nocase"),
        "ww" => ("ww", "noww"),
        "path" => ("path", "nopath"),
        "regex" => ("regex", "noregex"),
        _ => return raw.to_string(),
    };
    let mut kept = Vec::new();
    for token in tokens(raw) {
        if let Some((name, value)) = modifier(token)
            && (name == positive
                || name == negative
                || (option == "regex" && matches!(name.as_str(), "wildcards" | "nowildcards")))
        {
            if option == "path" && !value.is_empty() {
                // A path restriction remains useful even with filename-only matching.
                kept.push(token.to_string());
            } else if option == "regex" && !value.is_empty() {
                kept.push(if enabled {
                    token.to_string()
                } else {
                    value.to_string()
                });
            }
            continue;
        }
        kept.push(token.to_string());
    }
    let modifier = format!("{}:", if enabled { positive } else { negative });
    // Modes must precede text so the parser interprets all its terms consistently.
    // Path switches come last to override a preserved path:value restriction.
    if option == "path" {
        kept.push(modifier);
    } else {
        kept.insert(0, modifier);
    }
    kept.join(" ")
}

fn type_filter(name: &str) -> bool {
    matches!(
        name,
        "file" | "folder" | "ext" | "audio" | "doc" | "exe" | "pic" | "video" | "zip"
    )
}

pub fn set_preset(raw: &str, preset: usize) -> String {
    let Some(value) = PRESETS.get(preset) else {
        return raw.to_string();
    };
    let mut kept: Vec<_> = tokens(raw)
        .into_iter()
        .filter(|token| !modifier(token).is_some_and(|(name, _)| type_filter(&name)))
        .collect();
    if !value.is_empty() {
        kept.push(value);
    }
    kept.join(" ")
}

pub fn sync(ui: &crate::AppWindow, raw: &str) {
    // Keep controls responsive even while a regex or quoted value is incomplete.
    let mut case = false;
    let mut whole = false;
    let mut path = false;
    let mut regex = false;
    for token in tokens(raw) {
        if let Some((name, _)) = modifier(token) {
            match name.as_str() {
                "case" => case = true,
                "nocase" => case = false,
                "ww" => whole = true,
                "noww" => whole = false,
                "path" => path = true,
                "nopath" => path = false,
                "regex" => regex = true,
                "noregex" | "wildcards" => regex = false,
                _ => {}
            }
        }
    }
    if let Ok(query) = Query::parse(raw) {
        case = query.match_case;
        whole = query.match_whole_word;
        path = query.match_path;
        regex = query.mode == SearchMode::Regex;
    }
    ui.set_match_case(case);
    ui.set_match_whole_word(whole);
    ui.set_match_path(path);
    ui.set_regex_enabled(regex);
    ui.set_filter_preset(preset_index(raw));
}

fn preset_index(raw: &str) -> i32 {
    let types: Vec<_> = tokens(raw)
        .into_iter()
        .filter(|token| modifier(token).is_some_and(|(name, _)| type_filter(&name)))
        .collect();
    match types.as_slice() {
        [] => 0,
        [token] => PRESETS
            .iter()
            .position(|preset| token.eq_ignore_ascii_case(preset))
            .map_or(8, |index| i32::try_from(index).unwrap()),
        _ => 8,
    }
}

pub fn connect(ui: &crate::AppWindow) {
    let weak = ui.as_weak();
    ui.on_search_option(move |option, enabled| {
        if let Some(ui) = weak.upgrade() {
            let raw = set_option(&ui.get_query_text(), &option, enabled);
            ui.set_query_text(raw.clone().into());
            sync(&ui, &raw);
            ui.invoke_query_edited(raw.into());
        }
    });
    let weak = ui.as_weak();
    ui.on_preset_selected(move |preset| {
        if let Some(ui) = weak.upgrade() {
            let raw = set_preset(
                &ui.get_query_text(),
                usize::try_from(preset).unwrap_or(usize::MAX),
            );
            ui.set_query_text(raw.clone().into());
            sync(&ui, &raw);
            ui.invoke_query_edited(raw.into());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use toge_core::{Index, matcher::match_query};

    #[test]
    fn options_remove_conflicts_and_respect_quoted_literals() {
        let raw = "nocase: case: ww: noww: \"case:\" parent:\"/My Files\" Report";
        let raw = set_option(raw, "case", true);
        let raw = set_option(&raw, "ww", true);
        let query = Query::parse(&raw).unwrap();
        assert!(query.match_case && query.match_whole_word);
        assert_eq!(query.parent_filter.as_deref(), Some("/My Files"));
        assert!(raw.contains("\"case:\""));
        assert_eq!(raw.matches("ww:").count(), 1);
        assert_eq!(set_option(&raw, "ww", true), raw);
    }

    #[test]
    fn path_switch_preserves_the_directory_restriction() {
        let raw = set_option("path:\"/My Files\" report", "path", false);
        let query = Query::parse(&raw).unwrap();
        assert!(!query.match_path);
        assert_eq!(query.path_filter.as_deref(), Some("/My Files"));
        let raw = set_option(&raw, "path", true);
        assert!(Query::parse(&raw).unwrap().match_path);
        assert_eq!(raw.matches("path:\"/My Files\"").count(), 1);
    }

    #[test]
    fn regex_and_case_controls_change_real_matching() {
        let mut index = Index::new();
        index.insert("/fixture/Report.PDF", false);
        index.insert("/fixture/report.pdf", false);
        index.insert("/fixture/myreport.pdf", false);
        let raw = set_option("wildcards: ^Report\\.PDF$", "regex", true);
        assert_eq!(
            match_query(&index, &Query::parse(&raw).unwrap()),
            vec![0, 1]
        );
        let raw = set_option(&raw, "case", true);
        assert_eq!(match_query(&index, &Query::parse(&raw).unwrap()), vec![0]);
        let raw = set_option("regex:\"^Report\\.PDF$\"", "regex", false);
        assert_ne!(Query::parse(&raw).unwrap().mode, SearchMode::Regex);
        assert!(match_query(&index, &Query::parse(&raw).unwrap()).is_empty());
    }

    #[test]
    fn presets_replace_type_filters_without_losing_the_search() {
        let raw = "folder: audio: ext:txt case: parent:\"/My Files\" \"video:\" report";
        let raw = set_preset(raw, 6);
        let query = Query::parse(&raw).unwrap();
        assert!(!query.require_folder && !query.require_file);
        assert!(query.ext.unwrap().contains(&"webm".into()));
        assert!(query.match_case);
        assert_eq!(query.parent_filter.as_deref(), Some("/My Files"));
        assert!(raw.contains("\"video:\""));
        assert_eq!(preset_index(&raw), 6);
        let raw = set_preset(&raw, 0);
        assert_eq!(preset_index(&raw), 0);
        assert!(Query::parse(&raw).unwrap().ext.is_none());
        assert_eq!(preset_index("ext:pdf"), 8);
        assert_eq!(set_preset("ext:pdf", 8), "ext:pdf");
    }

    #[test]
    fn every_preset_matches_its_file_type_and_everything_resets_it() {
        let mut index = Index::new();
        for (name, dir) in [
            ("folder", true),
            ("track.mp3", false),
            ("report.odt", false),
            ("image.avif", false),
            ("clip.webm", false),
            ("data.zip", false),
        ] {
            index.insert(&format!("/fixture/{name}"), dir);
        }
        for (preset, expected) in [
            (1, vec![1, 2, 3, 4, 5]),
            (2, vec![0]),
            (3, vec![1]),
            (4, vec![2]),
            (5, vec![3]),
            (6, vec![4]),
            (7, vec![5]),
        ] {
            let raw = set_preset("", preset);
            assert_eq!(match_query(&index, &Query::parse(&raw).unwrap()), expected);
            assert_eq!(
                match_query(&index, &Query::parse(&set_preset(&raw, 0)).unwrap()).len(),
                6
            );
        }
    }
}
