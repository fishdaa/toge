use super::*;

#[test]
fn test_parse_positional_search() {
    let opts = NdlOptions::parse(["ndl".into(), "foo.txt".into()]).unwrap();
    assert_eq!(opts.search, "foo.txt");
}

#[test]
fn test_parse_multiple_positional_terms() {
    let opts = NdlOptions::parse(["ndl".into(), "foo".into(), "bar".into()]).unwrap();
    assert_eq!(opts.search, "foo bar");
}

#[test]
fn test_parse_regex_flag() {
    let opts = NdlOptions::parse(["ndl".into(), "-r".into(), "^foo\\d+".into()]).unwrap();
    assert_eq!(opts.regex, Some("^foo\\d+".into()));
}

#[test]
fn test_parse_case_flag() {
    let opts = NdlOptions::parse(["ndl".into(), "-i".into(), "ABC".into()]).unwrap();
    assert!(opts.case);
}

#[test]
fn test_parse_whole_word_flags() {
    let opts = NdlOptions::parse(["ndl".into(), "-w".into(), "foo".into()]).unwrap();
    assert!(opts.whole_word);

    let opts = NdlOptions::parse(["ndl".into(), "-ww".into(), "foo".into()]).unwrap();
    assert!(opts.whole_word);
}

#[test]
fn test_parse_match_path_flag() {
    let opts = NdlOptions::parse(["ndl".into(), "-p".into(), "docs".into()]).unwrap();
    assert!(opts.match_path);
}

#[test]
fn test_parse_max_results() {
    let opts = NdlOptions::parse(["ndl".into(), "-n".into(), "10".into(), "foo".into()]).unwrap();
    assert_eq!(opts.max_results, 10);
}

#[test]
fn test_parse_offset() {
    let opts = NdlOptions::parse(["ndl".into(), "-o".into(), "5".into(), "foo".into()]).unwrap();
    assert_eq!(opts.offset, 5);
}

#[test]
fn test_parse_show_columns() {
    let opts =
        NdlOptions::parse(["ndl".into(), "-size".into(), "-dm".into(), "foo".into()]).unwrap();
    assert!(opts.show_size);
    assert!(opts.show_modified);
}

#[test]
fn test_parse_sort() {
    let opts = NdlOptions::parse([
        "ndl".into(),
        "-sort".into(),
        "size-desc".into(),
        "foo".into(),
    ])
    .unwrap();
    assert_eq!(opts.sort, Some("size-desc".into()));
}

#[test]
fn test_parse_output_formats() {
    for (flag, expected) in [
        ("-csv", OutputFormat::Csv),
        ("-tsv", OutputFormat::Tsv),
        ("-txt", OutputFormat::Txt),
        ("-efu", OutputFormat::Efu),
    ] {
        let opts = NdlOptions::parse(["ndl".into(), flag.into(), "foo".into()]).unwrap();
        assert_eq!(opts.format, expected, "failed for {}", flag);
    }
}

#[test]
fn test_parse_export_file() {
    let opts = NdlOptions::parse([
        "ndl".into(),
        "-export-csv".into(),
        "out.csv".into(),
        "foo".into(),
    ])
    .unwrap();
    assert_eq!(opts.export_file, Some("out.csv".into()));
    assert_eq!(opts.format, OutputFormat::Csv);
}

#[test]
fn test_parse_display_flags() {
    let opts = NdlOptions::parse([
        "ndl".into(),
        "-no-header".into(),
        "-pause".into(),
        "-highlight".into(),
        "-highlight-color".into(),
        "7".into(),
        "foo".into(),
    ])
    .unwrap();
    assert!(opts.no_header);
    assert!(opts.pause);
    assert!(opts.highlight);
    assert_eq!(opts.highlight_color, 7);
}

#[test]
fn test_parse_meta_flags() {
    let opts = NdlOptions::parse(["ndl".into(), "-status".into()]).unwrap();
    assert!(opts.status);

    let opts = NdlOptions::parse(["ndl".into(), "-save-db".into()]).unwrap();
    assert!(opts.save_db);

    let opts = NdlOptions::parse(["ndl".into(), "-reindex".into()]).unwrap();
    assert!(opts.reindex);

    let opts = NdlOptions::parse(["ndl".into(), "-get-result-count".into(), "foo".into()]).unwrap();
    assert!(opts.get_result_count);

    let opts = NdlOptions::parse(["ndl".into(), "-get-total-size".into(), "foo".into()]).unwrap();
    assert!(opts.get_total_size);
}

#[test]
fn test_parse_sort_direction_flags_affect_search() {
    let opts = NdlOptions::parse([
        "ndl".into(),
        "-sort".into(),
        "size".into(),
        "-sort-ascending".into(),
        "foo".into(),
    ])
    .unwrap();
    assert!(opts.search.contains("sort:size-asc"));

    let opts = NdlOptions::parse([
        "ndl".into(),
        "-sort".into(),
        "size".into(),
        "-sort-descending".into(),
        "foo".into(),
    ])
    .unwrap();
    assert!(opts.search.contains("sort:size-desc"));
}

#[test]
fn test_parse_help_and_version() {
    let opts = NdlOptions::parse(["ndl".into(), "-h".into()]).unwrap();
    assert!(opts.help);

    let opts = NdlOptions::parse(["ndl".into(), "-version".into()]).unwrap();
    assert!(opts.version);
}

#[test]
fn test_parse_windows_style_attribute_flags() {
    let opts = NdlOptions::parse(["ndl".into(), "/ad".into(), "foo".into()]).unwrap();
    assert!(opts.search.contains("folder:"));

    let opts = NdlOptions::parse(["ndl".into(), "/a-d".into(), "foo".into()]).unwrap();
    assert!(opts.search.contains("file:"));
}

#[test]
fn test_parse_negative_max_results_is_error() {
    let result = NdlOptions::parse(["ndl".into(), "-n".into(), "-5".into(), "foo".into()]);
    assert!(result.is_err());
}
