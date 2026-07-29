//! Parse a string that carries ANSI SGR escapes + OSC 8 hyperlinks and
//! emit an HTML fragment with equivalent styled `<span>`s and `<a>`s.
//!
//! Uses `vte` for the actual VT-sequence parsing so we don't have to
//! hand-roll a parser. Only SGR and OSC 8 are interpreted — cursor
//! movement, screen clears, bell, etc. are silently dropped.
//! This is a code-block conversion, not a terminal emulator.

use crate::ansi_style::{CellStyle, Color};
use vte::{Params, Parser, Perform};

/// Default entry point — parse `source` and return an HTML fragment
/// with inline `<span style="…">` runs and OSC 8 `<a>` wrappers.
// Fragment only — no enclosing element, no `<pre>`. Callers that need
// the rendered result of cursor traffic must use `terminal` instead.
// [spec:mw:def:ansi.to-html.entry]
pub fn ansi_to_html(source: &str) -> String {
    let mut parser: Parser = Parser::new();
    let mut writer = HtmlWriter::default();
    parser.advance(&mut writer, source.as_bytes());
    writer.finish()
}

/// Annotate each ANSI escape in `source` as a muted
/// `<span class="ansi-esc">ESC[…m</span>` label and leave printable
/// characters alone (with HTML-escaping). Used for the "show source"
/// pane so readers can inspect the sequences without the browser
/// interpreting them.
// [spec:mw:syn:ansi.annotated.labels]
pub fn ansi_to_annotated_source(source: &str) -> String {
    let mut parser: Parser = Parser::new();
    let mut writer = AnnotatedSourceWriter::default();
    parser.advance(&mut writer, source.as_bytes());
    writer.finish()
}

// --- SGR → styled HTML spans ------------------------------------------
//
// `Color` / `CellStyle` / the palette helpers live in `crate::ansi_style`
// so the `terminal` grid renderer can share the same CSS mapping.

#[derive(Default)]
struct HtmlWriter {
    out: String,
    current: CellStyle,
    open_span: bool,
    open_link: Option<String>,
}

impl HtmlWriter {
    fn finish(mut self) -> String {
        self.close_span();
        self.close_link();
        self.out
    }

    // At most one span open at a time, never nested; opening is lazy so
    // a run of escapes with no text between them emits nothing, and a
    // plain style never produces an empty `<span style="">`.
    // [spec:mw:req:ansi.to-html.span-discipline]
    fn open_span(&mut self) {
        if self.open_span {
            return;
        }
        if self.current.is_plain() {
            return;
        }
        let style = self.current.css();
        self.out.push_str("<span style=\"");
        self.out.push_str(&style);
        self.out.push_str("\">");
        self.open_span = true;
    }

    fn close_span(&mut self) {
        if self.open_span {
            self.out.push_str("</span>");
            self.open_span = false;
        }
    }

    fn close_link(&mut self) {
        if self.open_link.take().is_some() {
            self.out.push_str("</a>");
        }
    }

    fn flush_pending_style(&mut self) {
        self.close_span();
        self.open_span();
    }

    // [spec:mw:req:ansi.to-html.text-and-controls]
    fn push_char(&mut self, c: char) {
        if !self.open_span && !self.current.is_plain() {
            self.open_span();
        }
        match c {
            '&' => self.out.push_str("&amp;"),
            '<' => self.out.push_str("&lt;"),
            '>' => self.out.push_str("&gt;"),
            '"' => self.out.push_str("&quot;"),
            _ => self.out.push(c),
        }
    }
}

impl Perform for HtmlWriter {
    fn print(&mut self, c: char) {
        self.push_char(c);
    }

    // [spec:mw:req:ansi.to-html.text-and-controls]
    fn execute(&mut self, byte: u8) {
        // C0 controls we want to preserve: newline, tab, carriage
        // return. Everything else gets dropped.
        match byte {
            b'\n' | b'\t' | b'\r' => {
                // Newlines happen outside of spans for clean output.
                // `current` is deliberately left intact, so the next
                // printable char reopens an equivalent span and colour
                // carries across lines the way a terminal shows it.
                self.close_span();
                self.out.push(byte as char);
            }
            _ => {}
        }
    }

    // [spec:mw:req:ansi.to-html.non-sgr-dropped]
    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // Only SGR (final `m`) changes visible style. Everything else is
        // cursor / screen manipulation — ignore. Dropping must be total:
        // no output, no placeholder, no mutation of `current`.
        if action != 'm' {
            return;
        }
        // Parameters mutate one running style left to right, so a later
        // param overrides an earlier one and a mid-list `0` resets what
        // the same escape already accumulated.
        // [spec:mw:sem:ansi.to-html.sgr-accumulation]
        let mut iter = params.iter().peekable();
        if iter.peek().is_none() {
            // Bare `ESC [m` is a reset.
            self.current = CellStyle::default();
            self.flush_pending_style();
            return;
        }
        // Only the first sub-parameter of each param is read, so the
        // colon-delimited colour forms are not honoured. Unrecognised
        // codes are skipped without aborting the rest of the sequence.
        // [spec:mw:req:ansi.to-html.sgr-params]
        while let Some(param) = iter.next() {
            let code = param.first().copied().unwrap_or(0);
            match code {
                0 => self.current = CellStyle::default(),
                1 => self.current.bold = true,
                2 => self.current.dim = true,
                3 => self.current.italic = true,
                4 => self.current.underline = true,
                7 => self.current.inverse = true,
                9 => self.current.strike = true,
                22 => {
                    self.current.bold = false;
                    self.current.dim = false;
                }
                23 => self.current.italic = false,
                24 => self.current.underline = false,
                27 => self.current.inverse = false,
                29 => self.current.strike = false,
                30..=37 => self.current.fg = Some(Color::Named(code as u8 - 30)),
                38 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        self.current.fg = Some(color);
                    }
                }
                39 => self.current.fg = None,
                40..=47 => self.current.bg = Some(Color::Named(code as u8 - 40)),
                48 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        self.current.bg = Some(color);
                    }
                }
                49 => self.current.bg = None,
                90..=97 => self.current.fg = Some(Color::Named(code as u8 - 90 + 8)),
                100..=107 => self.current.bg = Some(Color::Named(code as u8 - 100 + 8)),
                _ => {}
            }
        }
        self.flush_pending_style();
    }

    // Close the span before touching the anchor in both directions, so
    // spans nest strictly inside `<a>` and never straddle its boundary.
    // The URL is passed through verbatim — no scheme allowlist here.
    // [spec:mw:req:ansi.to-html.osc8]
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // OSC 8 hyperlinks: `ESC ] 8 ; params ; URL ST`.
        if params.first().map(|p| p.as_ref()) != Some(b"8".as_slice()) {
            return;
        }
        // params = [b"8", <params>, <url>].
        let url = params.get(2).copied().unwrap_or(b"");
        self.close_span();
        self.close_link();
        if url.is_empty() {
            return; // Closing sequence.
        }
        let url = String::from_utf8_lossy(url).into_owned();
        self.out.push_str("<a href=\"");
        for ch in url.chars() {
            match ch {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '"' => self.out.push_str("&quot;"),
                _ => self.out.push(ch),
            }
        }
        self.out.push_str("\">");
        self.open_link = Some(url);
    }
}

// Operands come off the caller's iterator, so an unrecognised selector
// swallows only itself and the remaining params fall back to the outer
// SGR loop. A truncated list leaves the colour unchanged.
// [spec:mw:req:ansi.to-html.extended-color]
fn parse_extended_color<'a, I>(iter: &mut I) -> Option<Color>
where
    I: Iterator<Item = &'a [u16]>,
{
    let kind = iter.next()?.first().copied()?;
    match kind {
        5 => {
            let n = iter.next()?.first().copied()?;
            Some(Color::Palette(n as u8))
        }
        2 => {
            let r = iter.next()?.first().copied()? as u8;
            let g = iter.next()?.first().copied()? as u8;
            let b = iter.next()?.first().copied()? as u8;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

// --- Annotated source pane --------------------------------------------

#[derive(Default)]
struct AnnotatedSourceWriter {
    out: String,
}

impl AnnotatedSourceWriter {
    fn finish(self) -> String {
        self.out
    }
}

// [spec:mw:syn:ansi.annotated.labels]
impl Perform for AnnotatedSourceWriter {
    fn print(&mut self, c: char) {
        match c {
            '&' => self.out.push_str("&amp;"),
            '<' => self.out.push_str("&lt;"),
            '>' => self.out.push_str("&gt;"),
            '"' => self.out.push_str("&quot;"),
            _ => self.out.push(c),
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | b'\t' | b'\r' => self.out.push(byte as char),
            _ => {
                self.out.push_str("<span class=\"ansi-esc\">^");
                self.out.push_str(&format!("{:02X}", byte));
                self.out.push_str("</span>");
            }
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        self.out.push_str("<span class=\"ansi-esc\">ESC[");
        let mut first = true;
        for param in params.iter() {
            if !first {
                self.out.push(';');
            }
            first = false;
            let mut sub_first = true;
            for &sp in param {
                if !sub_first {
                    self.out.push(':');
                }
                sub_first = false;
                self.out.push_str(&sp.to_string());
            }
        }
        self.out.push(action);
        self.out.push_str("</span>");
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        self.out.push_str("<span class=\"ansi-esc\">ESC ");
        for i in intermediates {
            self.out.push(*i as char);
        }
        self.out.push(byte as char);
        self.out.push_str("</span>");
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        self.out.push_str("<span class=\"ansi-esc\">OSC ");
        let mut first = true;
        for &p in params {
            if !first {
                self.out.push(';');
            }
            first = false;
            for &b in p {
                let c = b as char;
                match c {
                    '&' => self.out.push_str("&amp;"),
                    '<' => self.out.push_str("&lt;"),
                    '>' => self.out.push_str("&gt;"),
                    '"' => self.out.push_str("&quot;"),
                    _ if b.is_ascii_graphic() || b == b' ' => self.out.push(c),
                    _ => self.out.push_str(&format!("^{b:02X}")),
                }
            }
        }
        self.out.push_str(" ST</span>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:def:ansi.to-html.entry/test]
    // [spec:mw:req:ansi.to-html.span-discipline/test]
    // [spec:mw:sem:ansi.to-html.sgr-accumulation/test]
    #[test]
    fn sgr_red_text_becomes_colored_span() {
        let out = ansi_to_html("\x1b[31mred\x1b[0m plain");
        assert!(out.starts_with("<span style=\"color:"));
        assert!(out.contains("red</span> plain"));
    }

    // [spec:mw:req:ansi.to-html.sgr-params/test]
    // [spec:mw:req:ansi.style.css-emission/test]
    #[test]
    fn bold_italic_together() {
        let out = ansi_to_html("\x1b[1;3mbi\x1b[0m");
        assert!(out.contains("font-weight:700"));
        assert!(out.contains("font-style:italic"));
        assert!(out.contains("bi</span>"));
    }

    // [spec:mw:def:ansi.style.color/test]
    // [spec:mw:req:ansi.to-html.extended-color/test]
    #[test]
    fn truecolor_fg() {
        let out = ansi_to_html("\x1b[38;2;255;100;50mhot\x1b[0m");
        assert!(out.contains("color:#ff6432"));
    }

    // [spec:mw:sem:ansi.style.palette-resolution/test]
    #[test]
    fn ansi256_palette() {
        // 201 is the 6-6-cube magenta near the corner.
        let out = ansi_to_html("\x1b[38;5;201mx\x1b[0m");
        assert!(out.contains("color:#"));
    }

    // [spec:mw:req:ansi.to-html.osc8/test]
    #[test]
    fn osc8_hyperlink_roundtrips() {
        let out = ansi_to_html("\x1b]8;;https://silo.dev\x1b\\click\x1b]8;;\x1b\\");
        assert!(out.contains("<a href=\"https://silo.dev\">"));
        assert!(out.contains("click"));
        assert!(out.contains("</a>"));
    }

    // [spec:mw:req:ansi.to-html.text-and-controls/test]
    #[test]
    fn html_entities_escaped() {
        let out = ansi_to_html("<x & y>");
        assert_eq!(out, "&lt;x &amp; y&gt;");
    }

    // [spec:mw:req:ansi.to-html.non-sgr-dropped/test]
    #[test]
    fn non_sgr_csi_is_dropped() {
        // Cursor to home and clear screen should not leak into output.
        let out = ansi_to_html("\x1b[H\x1b[2Jhello");
        assert_eq!(out, "hello");
    }

    // [spec:mw:syn:ansi.annotated.labels/test]
    #[test]
    fn annotated_source_labels_escape_sequences() {
        let source = "\x1b[31mred\x1b[0m";
        let out = ansi_to_annotated_source(source);
        assert!(out.contains("<span class=\"ansi-esc\">ESC[31m</span>"));
        assert!(out.contains("red"));
        assert!(out.contains("<span class=\"ansi-esc\">ESC[0m</span>"));
    }
}
