//! Post-processing pass that converts `<text>` elements in generated
//! SVG to outlined glyph `<path>`s with an invisible `<text>` overlay
//! for selection / accessibility. Same pattern as `mathml-rs` uses for
//! its math SVG output — self-contained rendering that doesn't depend
//! on whatever fonts the viewer has, while text selection and
//! screen-reader semantics survive.
//!
//! Applied to graphviz and pikchr output after the security sanitiser.
//! Falls back to leaving the `<text>` unchanged on any parse failure
//! (unknown anchor, missing font glyph, malformed attributes).

use std::sync::LazyLock;

use mathml_core::font::STIX_TWO_TEXT_ROMAN_DATA;
use mathml_core::glyph::extract_glyph_path;
use ttf_parser::Face;

// adv[impl output.text-as-paths]
static FACE: LazyLock<Face<'static>> = LazyLock::new(|| {
    Face::parse(STIX_TWO_TEXT_ROMAN_DATA, 0)
        .expect("STIX Two Text Roman (bundled with mathml-core) must parse")
});

/// Replace every `<text …>content</text>` in `svg` with outlined
/// glyph paths followed by an invisible `<text>` for selection.
/// `<text>` elements we can't understand (unknown attributes, missing
/// glyphs) are left alone.
pub fn text_to_paths(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut i = 0;
    while i < svg.len() {
        let rest_bytes = &svg.as_bytes()[i..];
        if rest_bytes[0] == b'<' && starts_with_ci(rest_bytes, b"<text") {
            if let Some((consumed, replacement)) = try_convert_text(rest_bytes) {
                out.push_str(&replacement);
                i += consumed;
                continue;
            }
        }
        // Copy one UTF-8 codepoint so multi-byte characters in labels
        // round-trip correctly.
        let ch = svg[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Try to consume a `<text …>CONTENT</text>` block starting at the
/// beginning of `bytes`. Returns `(bytes_consumed, replacement_svg)`
/// on success.
fn try_convert_text(bytes: &[u8]) -> Option<(usize, String)> {
    let open_close = bytes.iter().position(|&b| b == b'>')?;
    let tag_attrs = core::str::from_utf8(&bytes[5..open_close]).ok()?; // skip "<text"
    let after_open = open_close + 1;
    // Find matching </text>.
    let content_end_rel = find_ci(&bytes[after_open..], b"</text>")?;
    let content = core::str::from_utf8(&bytes[after_open..after_open + content_end_rel]).ok()?;
    let total = after_open + content_end_rel + b"</text>".len();

    // Parse attributes we care about.
    let x = parse_f64_attr(tag_attrs, "x")?;
    let y = parse_f64_attr(tag_attrs, "y")?;
    let font_size = parse_f64_attr(tag_attrs, "font-size")?;
    let anchor = parse_str_attr(tag_attrs, "text-anchor").unwrap_or("start");
    let fill = parse_str_attr(tag_attrs, "fill").unwrap_or("currentColor");

    // Unescape the content — graphviz emits plain text or XML entities
    // (`&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`).
    let content = unescape_xml(content);
    if content.is_empty() {
        return None;
    }

    let face = &*FACE;
    let upem = face.units_per_em();
    if upem == 0 {
        return None;
    }
    let scale = font_size / upem as f64;

    // First pass: compute total advance so we can anchor-adjust.
    let mut total_advance: f64 = 0.0;
    let mut glyphs: Vec<(ttf_parser::GlyphId, f64)> = Vec::with_capacity(content.len());
    for ch in content.chars() {
        if ch.is_control() {
            continue;
        }
        let glyph_id = match face.glyph_index(ch) {
            Some(g) => g,
            None => {
                // Unknown glyph — bail so the caller emits the original
                // <text> and the browser's fallback font handles it.
                return None;
            }
        };
        let advance = face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64 * scale;
        glyphs.push((glyph_id, advance));
        total_advance += advance;
    }
    if glyphs.is_empty() {
        return None;
    }

    let start_x = match anchor {
        "middle" => x - total_advance / 2.0,
        "end" => x - total_advance,
        _ => x, // "start" or any other value
    };

    // Build replacement: paths + transparent overlay text.
    let mut out = String::with_capacity(256 + content.len() * 80);
    let mut cursor_x = start_x;
    for (glyph_id, advance) in glyphs {
        if let Some(path) = extract_glyph_path(face, glyph_id) {
            out.push_str("<path d=\"");
            out.push_str(&path.d);
            out.push_str("\" transform=\"translate(");
            write_f64(&mut out, cursor_x);
            out.push(' ');
            write_f64(&mut out, y);
            out.push_str(") scale(");
            write_f64(&mut out, scale);
            out.push(' ');
            out.push('-');
            write_f64(&mut out, scale);
            out.push_str(")\" fill=\"");
            out.push_str(&xml_escape_attr(fill));
            out.push_str("\"/>");
        }
        cursor_x += advance;
    }
    // Invisible selectable overlay — preserves copy/paste + screen readers.
    out.push_str("<text x=\"");
    write_f64(&mut out, x);
    out.push_str("\" y=\"");
    write_f64(&mut out, y);
    out.push_str("\" font-size=\"");
    write_f64(&mut out, font_size);
    out.push_str("\" fill=\"transparent\" text-anchor=\"");
    out.push_str(anchor);
    out.push_str("\">");
    out.push_str(&xml_escape_text(&content));
    out.push_str("</text>");

    Some((total, out))
}

fn parse_f64_attr(attrs: &str, name: &str) -> Option<f64> {
    parse_str_attr(attrs, name).and_then(|v| v.trim().parse::<f64>().ok())
}

fn parse_str_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    // Look for `name=` preceded by whitespace or start-of-string.
    let mut search_from = 0;
    while let Some(pos) = attrs[search_from..].find(name) {
        let abs = search_from + pos;
        let preceded_by_ws = abs == 0
            || matches!(
                attrs.as_bytes()[abs - 1],
                b' ' | b'\t' | b'\n' | b'\r'
            );
        let after = abs + name.len();
        let followed_by_eq_ws = after < attrs.len()
            && (attrs.as_bytes()[after] == b'='
                || matches!(attrs.as_bytes()[after], b' ' | b'\t' | b'\n' | b'\r'));
        if preceded_by_ws && followed_by_eq_ws {
            // Skip whitespace + `=` + whitespace.
            let mut p = after;
            while p < attrs.len()
                && matches!(attrs.as_bytes()[p], b' ' | b'\t' | b'\n' | b'\r')
            {
                p += 1;
            }
            if p >= attrs.len() || attrs.as_bytes()[p] != b'=' {
                search_from = after;
                continue;
            }
            p += 1;
            while p < attrs.len()
                && matches!(attrs.as_bytes()[p], b' ' | b'\t' | b'\n' | b'\r')
            {
                p += 1;
            }
            if p >= attrs.len() {
                return None;
            }
            let quote = attrs.as_bytes()[p];
            if quote != b'"' && quote != b'\'' {
                return None;
            }
            p += 1;
            let value_start = p;
            while p < attrs.len() && attrs.as_bytes()[p] != quote {
                p += 1;
            }
            if p >= attrs.len() {
                return None;
            }
            return Some(&attrs[value_start..p]);
        }
        search_from = abs + name.len();
    }
    None
}

fn write_f64(out: &mut String, v: f64) {
    // Round to 2 decimal places for compact output.
    let rounded = (v * 100.0).round() / 100.0;
    if rounded == rounded.trunc() {
        use core::fmt::Write;
        let _ = write!(out, "{}", rounded as i64);
    } else {
        use core::fmt::Write;
        let _ = write!(out, "{rounded:.2}");
    }
}

fn starts_with_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack[..needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn find_ci(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| starts_with_ci(&haystack[i..], needle))
}

fn unescape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(end) = bytes[i..].iter().position(|&b| b == b';') {
                let entity = &s[i + 1..i + end];
                let replaced = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => None,
                };
                if let Some(c) = replaced {
                    out.push(c);
                    i += end + 1;
                    continue;
                }
            }
        }
        // Pick the full utf-8 char.
        let ch = s[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn xml_escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn xml_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // adv[verify output.text-as-paths]
    #[test]
    fn simple_text_converts_to_path_plus_overlay() {
        let svg = r#"<g><text text-anchor="middle" x="50" y="20" font-size="14" fill="black">Hi</text></g>"#;
        let out = text_to_paths(svg);
        assert!(out.contains("<path d="), "got: {out}");
        assert!(out.contains("fill=\"black\""), "got: {out}");
        // Overlay preserves original text for selection.
        assert!(out.contains("fill=\"transparent\""), "got: {out}");
        assert!(out.contains(">Hi</text>"), "got: {out}");
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn unknown_anchor_treated_as_start() {
        let svg = r#"<text x="0" y="0" font-size="14">A</text>"#;
        let out = text_to_paths(svg);
        assert!(out.contains("<path d="), "got: {out}");
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn missing_glyph_leaves_original_text() {
        // STIX Two Text Roman doesn't cover this CJK codepoint, so the
        // converter bails and the original <text> is preserved verbatim
        // so the browser's fallback font can render it.
        let svg = r#"<text x="0" y="0" font-size="14">漢</text>"#;
        let out = text_to_paths(svg);
        assert!(out.contains("<text"), "got: {out}");
        assert!(out.contains("漢"), "got: {out}");
        assert!(!out.contains("<path d="), "got: {out}");
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn xml_entities_in_content_are_decoded_then_reescaped() {
        let svg = r#"<text x="0" y="0" font-size="14">a&amp;b</text>"#;
        let out = text_to_paths(svg);
        // The overlay text should contain the original "a&amp;b" form.
        assert!(out.contains("a&amp;b"), "got: {out}");
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn no_text_elements_unchanged() {
        let svg = "<svg><g><circle/></g></svg>";
        assert_eq!(text_to_paths(svg), svg);
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn multiple_texts_all_converted() {
        let svg = concat!(
            r#"<text x="0" y="0" font-size="14">A</text>"#,
            r#"<text x="10" y="10" font-size="14">B</text>"#,
        );
        let out = text_to_paths(svg);
        assert_eq!(out.matches("<path d=").count(), 2, "got: {out}");
        assert_eq!(out.matches("<text").count(), 2, "got: {out}"); // two overlay texts
    }
}
