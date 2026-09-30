use super::{AtomicU64, Command, Duration, Kind, Ordering, Path, ScratchDir, fs, kind, run_stdout};
use std::sync::OnceLock;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MAX_COLUMNS: usize = 1_024;

pub(super) struct TextPreview {
    pub lines: Vec<Line>,
    pub source: String,
    pub font: String,
    pub columns: i32,
    pub label: String,
    pub wrap: bool,
}

pub(super) struct Line {
    pub number: i32,
    pub text: String,
    pub tokens: Vec<crate::PreviewToken>,
}

pub(super) fn model(lines: Vec<Line>) -> slint::ModelRc<crate::PreviewLine> {
    slint::ModelRc::new(slint::VecModel::from(
        lines
            .into_iter()
            .map(|line| crate::PreviewLine {
                number: line.number,
                text: line.text.into(),
                tokens: slint::ModelRc::new(slint::VecModel::from(line.tokens)),
            })
            .collect::<Vec<_>>(),
    ))
}

fn font_family() -> &'static str {
    static FAMILY: OnceLock<String> = OnceLock::new();
    FAMILY.get_or_init(|| {
        let find = || {
            let scratch = ScratchDir::new().ok()?;
            let output = scratch.join("font-family");
            let mut command = Command::new("fc-match");
            command.args(["--format", "%{family}", "monospace"]);
            // The font is shared by all previews; finish this short, bounded lookup
            // even if the first file selection changes while Fontconfig initializes.
            run_stdout(
                command,
                &output,
                4096,
                Duration::from_secs(1),
                0,
                &AtomicU64::new(0),
                "Fontconfig unavailable",
                "Cannot resolve monospace font",
            )
            .ok()?;
            let font = fs::read_to_string(output)
                .ok()?
                .split(',')
                .next()?
                .trim()
                .to_owned();
            (!font.is_empty()).then_some(font)
        };
        find().unwrap_or_else(|| "DejaVu Sans Mono".into())
    })
}

fn color_codes(sequence: &str, current: &mut slint::Color, foreground: slint::Color) {
    let codes = sequence
        .split(';')
        .filter_map(|code| code.parse::<u8>().ok())
        .collect::<Vec<_>>();
    match codes.as_slice() {
        [38, 2, red, green, blue] => *current = slint::Color::from_rgb_u8(*red, *green, *blue),
        [] | [0 | 39] => *current = foreground,
        _ => {}
    }
}

fn tokens(line: &str, foreground: slint::Color) -> Vec<crate::PreviewToken> {
    let mut color = foreground;
    let mut rest = line;
    let mut column = 0;
    let mut result = Vec::new();
    while !rest.is_empty() {
        let end = rest.find('\u{1b}').unwrap_or(rest.len());
        let value = &rest[..end];
        if !value.is_empty() {
            result.push(crate::PreviewToken {
                text: value.into(),
                column,
                color,
            });
            column += i32::try_from(value.width()).unwrap_or(0);
        }
        rest = &rest[end..];
        if let Some(sequence) = rest.strip_prefix("\u{1b}[")
            && let Some(end) = sequence.find('m')
        {
            color_codes(&sequence[..end], &mut color, foreground);
            rest = &sequence[end + 1..];
            continue;
        }
        if !rest.is_empty() {
            rest = &rest[1..];
        }
    }
    result
}

pub(super) fn prepare(
    contents: &str,
    path: &Path,
    dark: bool,
    request: u64,
    serial: &AtomicU64,
) -> TextPreview {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let code = !matches!(
        ext.as_str(),
        "txt"
            | "text"
            | "md"
            | "markdown"
            | "log"
            | "csv"
            | "tsv"
            | "doc"
            | "docx"
            | "odt"
            | "rtf"
            | "xlsx"
            | "ods"
    ) && kind(path.to_str().unwrap_or("")) == Kind::Text;
    let foreground = if dark {
        slint::Color::from_rgb_u8(220, 223, 228)
    } else {
        slint::Color::from_rgb_u8(40, 44, 52)
    };
    let mut display_lines = Vec::new();
    let mut columns = 0;
    let mut clipped = false;
    for source in contents.split('\n') {
        let source = source.strip_suffix('\r').unwrap_or(source);
        let mut display = String::new();
        let mut count = 0;
        for ch in source.chars() {
            let width = if ch == '\t' {
                4 - count % 4
            } else {
                ch.width().unwrap_or(1)
            };
            if count + width > MAX_COLUMNS {
                clipped = true;
                display.push_str(" … [line truncated]");
                break;
            }
            if ch == '\t' {
                display.push_str(&" ".repeat(width));
            } else if ch.is_control() {
                display.push('�');
            } else {
                display.push(ch);
            }
            count += width;
        }
        columns = columns.max(display.width());
        display_lines.push(display);
    }
    let plain = display_lines.join("\n");
    let colored = code
        .then(|| highlight(&plain, path, dark, request, serial))
        .flatten();
    let highlighted = colored
        .as_deref()
        .map(|value| value.lines().collect::<Vec<_>>());
    let lines = display_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let mut spans = highlighted
                .as_ref()
                .and_then(|lines| lines.get(index))
                .map(|value| tokens(value, foreground))
                .unwrap_or_default();
            // Never let tool decorations or an unexpected escape alter the source.
            if spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
                != *line
            {
                spans = vec![crate::PreviewToken {
                    text: line.clone().into(),
                    column: 0,
                    color: foreground,
                }];
            }
            Line {
                number: i32::try_from(index + 1).unwrap_or(i32::MAX),
                text: line.clone(),
                tokens: spans,
            }
        })
        .collect::<Vec<_>>();
    let label = format!(
        "{} · {} lines{}",
        if code {
            if colored.is_some() {
                "Code"
            } else {
                "Code (plain)"
            }
        } else {
            "Text"
        },
        lines.len(),
        if clipped {
            " · long lines truncated"
        } else {
            ""
        }
    );
    TextPreview {
        lines,
        source: contents.to_owned(),
        font: font_family().to_owned(),
        columns: i32::try_from(columns).unwrap_or(i32::MAX),
        label,
        wrap: !code && !matches!(ext.as_str(), "csv" | "tsv" | "xlsx" | "ods"),
    }
}

fn highlight(
    contents: &str,
    path: &Path,
    dark: bool,
    request: u64,
    serial: &AtomicU64,
) -> Option<String> {
    let scratch = ScratchDir::new().ok()?;
    let source = scratch.join("source");
    fs::write(&source, contents).ok()?;
    let output = scratch.join("highlighted");
    for executable in ["bat", "batcat"] {
        let mut command = Command::new(executable);
        command
            .args([
                "--no-config",
                "--color=always",
                "--style=plain",
                "--paging=never",
                "--wrap=never",
                "--theme",
            ])
            .arg(if dark { "OneHalfDark" } else { "OneHalfLight" })
            .arg("--file-name")
            .arg(path)
            .arg(&source)
            .env("COLORTERM", "truecolor")
            .env("TERM", "xterm-256color");
        if run_stdout(
            command,
            &output,
            1024 * 1024,
            Duration::from_secs(2),
            request,
            serial,
            "bat unavailable",
            "Cannot highlight code",
        )
        .is_ok()
        {
            return fs::read_to_string(output).ok();
        }
        if serial.load(Ordering::Acquire) != request {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::tools;
    #[test]
    fn code_preserves_lines_indent_and_uses_syntax_colors() {
        let preview = prepare(
            "fn main() {\n\tlet message = \"<&>\";\n}\n",
            Path::new("main.rs"),
            false,
            1,
            &AtomicU64::new(1),
        );
        assert!(!preview.wrap);
        assert_eq!(preview.lines.len(), 4);
        assert_eq!(preview.lines[1].text.as_str(), "    let message = \"<&>\";");
        let tokens = &preview.lines[1].tokens;
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            preview.lines[1].text.as_str()
        );
        if tools::available(std::ffi::OsStr::new("bat"))
            || tools::available(std::ffi::OsStr::new("batcat"))
        {
            assert!(tokens.iter().any(|token| token.color != tokens[0].color));
        }
    }
    #[test]
    fn plain_text_wraps_and_long_lines_are_bounded() {
        let preview = prepare(
            &format!("a\n\n{}", "é".repeat(10_000)),
            Path::new("notes.txt"),
            true,
            1,
            &AtomicU64::new(1),
        );
        assert!(preview.wrap);
        assert_eq!(preview.lines.len(), 3);
        assert_eq!(preview.lines[1].text.as_str(), "");
        assert!(preview.columns < 1_100);
        assert!(preview.label.contains("truncated"));
    }
    #[test]
    fn ansi_colors_and_wide_characters_preserve_columns() {
        let spans = tokens(
            "a\x1b[38;2;1;2;3m界\x1b[0m<&>",
            slint::Color::from_rgb_u8(0, 0, 0),
        );
        assert_eq!(spans[1].column, 1);
        assert_eq!(spans[2].column, 3);
        assert_eq!(spans[1].color, slint::Color::from_rgb_u8(1, 2, 3));
    }
}
