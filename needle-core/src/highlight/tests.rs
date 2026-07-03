use super::*;

#[test]
fn test_render_ansi_wraps_marked_terms() {
    let rendered = render_ansi("*foo*.txt", 2); // green
    assert!(rendered.contains("\x1b[32mfoo\x1b[0m"));
    assert!(!rendered.contains('*'));
}

#[test]
fn test_render_ansi_escapes_literal_asterisks() {
    let rendered = render_ansi("**foo**.txt", 2);
    assert!(rendered.contains("*foo*"));
    assert!(!rendered.contains("\x1b[32m*foo*\x1b[0m"));
}

#[test]
fn test_strip_ansi_removes_codes() {
    let raw = "\x1b[32mfoo\x1b[0m".to_string();
    assert_eq!(strip_ansi(&raw), "foo");
}
