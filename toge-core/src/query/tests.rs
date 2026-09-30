use super::*;

#[test]
fn quoted_filter_values_and_literal_modifiers_are_distinct() {
    let query = Query::parse("parent:\"/My Files\" \"folder:\" \"two words\"").unwrap();
    assert_eq!(query.parent_filter.as_deref(), Some("/My Files"));
    assert!(!query.require_folder);
    assert_eq!(
        query.terms,
        vec![
            TextTerm::Substring("folder:".into()),
            TextTerm::Substring("two words".into())
        ]
    );
    assert!(
        Query::parse("parent:\"/My Files")
            .unwrap_err()
            .to_string()
            .contains("unterminated")
    );
    for name in ["parent", "infolder", "nosubfolders"] {
        assert_eq!(
            Query::parse(&format!("{name}:/tmp"))
                .unwrap()
                .parent_filter
                .as_deref(),
            Some("/tmp")
        );
        assert!(Query::parse(&format!("{name}:")).is_err());
    }
}

#[test]
fn unsupported_filters_and_attributes_report_errors() {
    for raw in [
        "child:foo",
        "empty:",
        "diacritics:",
        "attrib:R",
        "attrib:S",
        "attrib:X",
        "attrib:",
        "ext:",
    ] {
        assert!(Query::parse(raw).is_err(), "silently accepted {raw}");
    }
    let attributes = Query::parse("attrib:dh").unwrap().attributes.unwrap();
    assert_eq!(attributes.dir, Some(true));
    assert_eq!(attributes.hidden, Some(true));
}

#[test]
fn depth_filters_validate_ranges_and_overflow() {
    for (raw, min, max) in [
        ("depth:2", Some(2), Some(2)),
        ("parents:2..4", Some(2), Some(4)),
        ("depth:>2", Some(3), None),
        ("depth:<=2", None, Some(2)),
    ] {
        assert_eq!(
            Query::parse(raw).unwrap().depth,
            Some(RangeFilter { min, max })
        );
    }
    for raw in [
        "depth:",
        "depth:<0",
        "depth:4-2",
        "depth:>18446744073709551615",
        "depth:-1",
        "depth:1mb",
    ] {
        assert!(Query::parse(raw).is_err(), "accepted invalid range: {raw}");
    }
}

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
fn test_parse_date_modified_explicit_day() {
    let q = Query::parse("dm:2025-01-01 foo").unwrap();
    assert_eq!(
        q.date_modified,
        Some(RangeFilter {
            min: Some(1_735_689_600),
            max: Some(1_735_775_999),
        })
    );
}

#[test]
fn test_parse_date_modified_invalid_value_errors() {
    let err = Query::parse("dm:not-a-date foo").unwrap_err();
    assert!(err.to_string().contains("invalid date"));
}

#[test]
fn test_parse_date_modified_invalid_calendar_day_errors() {
    let err = Query::parse("dm:2025-02-29 foo").unwrap_err();
    assert!(err.to_string().contains("invalid date"));
}

#[test]
fn test_parse_date_modified_invalid_month_errors() {
    let err = Query::parse("dm:2025-13-01 foo").unwrap_err();
    assert!(err.to_string().contains("invalid date"));
}

#[test]
fn test_parse_date_modified_missing_day_errors() {
    let err = Query::parse("dm:2025-01 foo").unwrap_err();
    assert!(err.to_string().contains("invalid date"));
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
fn test_parse_file_type_macro_pic_uses_webp() {
    let q = Query::parse("pic: image").unwrap();
    let exts = q.ext.unwrap();
    assert!(exts.contains(&"webp".into()));
    assert!(!exts.contains(&"webm".into()));
}

#[test]
fn test_parse_invalid_regex_reports_error() {
    // Unclosed group should produce a parse error.
    let result = Query::parse("regex:(foo");
    assert!(result.is_err());
}

#[test]
fn test_parse_semantically_invalid_regex_reports_error() {
    let result = Query::parse("regex:(?P< foo");
    assert!(result.is_err());
}

#[test]
fn test_parse_invalid_sort_reports_error() {
    let err = Query::parse("sort:not-a-sort foo").unwrap_err();
    assert!(err.to_string().contains("unknown sort"));
}

#[test]
fn test_parse_sort_function() {
    let q = Query::parse("sort:size-desc foo").unwrap();
    assert_eq!(q.sort, Sort::SizeDesc);
}

#[test]
fn test_parse_overlong_regex_reports_error() {
    let pattern = "a".repeat(513);
    let err = Query::parse(&format!("regex:{pattern}")).unwrap_err();
    assert!(err.to_string().contains("regex too long"));
}

#[test]
fn test_parse_deeply_nested_regex_reports_error() {
    let err = Query::parse("regex:(((((((((a)))))))))").unwrap_err();
    assert!(err.to_string().contains("regex too complex"));
}

#[test]
fn test_parse_many_alternations_regex_reports_error() {
    let pattern = (0..34)
        .map(|i| format!("p{i}"))
        .collect::<Vec<_>>()
        .join("|");
    let err = Query::parse(&format!("regex:{pattern}")).unwrap_err();
    assert!(err.to_string().contains("regex too complex"));
}
