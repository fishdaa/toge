//! Result highlighting helpers.

/// Render a highlighted string by converting `*term*` markers to ANSI escapes.
pub fn render_ansi(input: &str, color: u8) -> String {
    let _ = (input, color);
    todo!()
}

/// Convert an ANSI-rendered string back to plain text.
pub fn strip_ansi(input: &str) -> String {
    let _ = input;
    todo!()
}

#[cfg(test)]
mod tests;
