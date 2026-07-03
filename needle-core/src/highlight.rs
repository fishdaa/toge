//! Result highlighting helpers.

/// Render a highlighted string by converting `*term*` markers to ANSI escapes.
pub fn render_ansi(input: &str, color: u8) -> String {
    let mut out = String::with_capacity(input.len() + 20);
    let mut chars = input.chars().peekable();
    let open = format!("\x1b[3{}m", color % 8);
    let close = "\x1b[0m";

    while let Some(c) = chars.next() {
        if c == '*' {
            if chars.peek() == Some(&'*') {
                chars.next();
                out.push('*');
                continue;
            }
            let mut term = String::new();
            let mut closed = false;
            for hc in chars.by_ref() {
                if hc == '*' {
                    closed = true;
                    break;
                }
                term.push(hc);
            }
            if closed && !term.is_empty() {
                out.push_str(&open);
                out.push_str(&term);
                out.push_str(close);
            } else {
                out.push('*');
                out.push_str(&term);
            }
        } else {
            out.push(c);
        }
    }

    out
}

/// Convert an ANSI-rendered string back to plain text.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for nc in chars.by_ref() {
                if nc == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests;
