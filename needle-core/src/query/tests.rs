use super::*;

#[test]
fn test_parse_simple_substring() {
    let q = Query::parse("foo").unwrap();
    assert_eq!(q.mode, SearchMode::Substring);
    assert!(!q.match_case);
    assert!(!q.match_path);
    assert_eq!(q.terms, vec![TextTerm::Substring("foo".into())]);
}

#[test]
fn test_parse_wildcard_mode_auto_detected() {
    let q = Query::parse("*.mp3").unwrap();
    assert_eq!(q.mode, SearchMode::Wildcard);
    assert_eq!(q.terms, vec![TextTerm::Wildcard("*.mp3".into())]);
}

#[test]
fn test_parse_regex_prefix() {
    let q = Query::parse("regex:^foo\\d+").unwrap();
    assert_eq!(q.mode, SearchMode::Regex);
    assert_eq!(q.terms, vec![TextTerm::Regex("^foo\\d+".into())]);
}

#[test]
fn test_parse_or_operator() {
    let q = Query::parse("foo|bar").unwrap();
    assert_eq!(
        q.terms,
        vec![TextTerm::Or(vec![
            TextTerm::Substring("foo".into()),
            TextTerm::Substring("bar".into())
        ])]
    );
}

#[test]
fn test_parse_and_operator_space() {
    let q = Query::parse("foo bar").unwrap();
    assert_eq!(
        q.terms,
        vec![
            TextTerm::Substring("foo".into()),
            TextTerm::Substring("bar".into())
        ]
    );
}

#[test]
fn test_parse_case_modifier() {
    let q = Query::parse("case:ABC").unwrap();
    assert!(q.match_case);
}

#[test]
fn test_parse_file_folder_modifiers() {
    let q = Query::parse("file: foo").unwrap();
    assert!(q.require_file);

    let q = Query::parse("folder: foo").unwrap();
    assert!(q.require_folder);
}

#[test]
fn test_parse_path_modifier() {
    let q = Query::parse("path:docs foo").unwrap();
    assert!(q.match_path);
    assert_eq!(q.path_filter, Some("docs".into()));
}

#[test]
fn test_parse_ext_function() {
    let q = Query::parse("ext:txt;pdf foo").unwrap();
    assert_eq!(q.ext, Some(vec!["txt".into(), "pdf".into()]));
}

#[test]
fn test_parse_size_comparison() {
    let q = Query::parse("size:>1mb foo").unwrap();
    assert!(q.size.is_some());
    let size = q.size.unwrap();
    // "Greater than" is strict: 1 MB itself is excluded, so the minimum is 1_000_001.
    assert_eq!(size.min, Some(1_000_001));
    assert_eq!(size.max, None);
}

#[test]
fn test_parse_size_range() {
    let q = Query::parse("size:1mb..10mb foo").unwrap();
    let size = q.size.unwrap();
    assert_eq!(size.min, Some(1_000_000));
    assert_eq!(size.max, Some(10_000_000));
}

#[test]
fn test_parse_strictly_less_than_zero_size_is_error() {
    let err = Query::parse("size:<0 foo").unwrap_err();
    assert!(err.to_string().contains("strictly less than zero"));
}

#[test]
fn test_parse_date_modified_today() {
    let q = Query::parse("dm:today foo").unwrap();
    assert!(q.date_modified.is_some());
}

#[test]
fn test_parse_date_created_today() {
    let q = Query::parse("dc:today foo").unwrap();
    assert!(q.date_created.is_some());
}

#[test]
fn test_parse_date_accessed_today() {
    let q = Query::parse("da:today foo").unwrap();
    assert!(q.date_accessed.is_some());
}

#[test]
fn test_parse_file_type_macro_doc() {
    let q = Query::parse("doc: report").unwrap();
    assert!(q.ext.is_some());
    let exts = q.ext.unwrap();
    assert!(exts.contains(&"pdf".into()));
    assert!(exts.contains(&"txt".into()));
}

#[test]
fn test_parse_invalid_regex_reports_error() {
    // Unclosed group should produce a parse error.
    let result = Query::parse("regex:(foo");
    assert!(result.is_err());
}

#[test]
fn test_parse_sort_function() {
    let q = Query::parse("sort:size-desc foo").unwrap();
    assert_eq!(q.sort, Sort::SizeDesc);
}
