//! Post-processing pass that converts `<text>` elements in generated
//! SVG to outlined glyph `<path>`s with an invisible `<text>` overlay
//! for selection / accessibility. Same pattern as `mathml-rs` uses for
//! its math SVG output — self-contained rendering that doesn't depend
//! on whatever fonts the viewer has, while text selection and
//! screen-reader semantics survive.
//!
//! Applied to graphviz and pikchr output after the security sanitiser.
//! Falls back to leaving the `<text>` unchanged when we can't render a
//! given input (unknown attributes, or a codepoint absent from all
//! three bundled faces).

use core::fmt::Write as _;
use std::sync::LazyLock;

use ttf_parser::{Face, GlyphId, OutlineBuilder};

// Red Hat's Liberation family (SIL OFL 1.1). Metric-compatible drop-in
// replacements for Times / Helvetica / Courier, which is exactly what
// graphviz and pikchr name in their emitted `font-family` attributes.
// Using metric-compatible fonts means advances line up with whatever
// the diagram engine used for its own layout calculations, so
// `text-anchor="middle"` labels stay centred inside their shapes.
pub const LIBERATION_SERIF_DATA: &[u8] = include_bytes!("../fonts/LiberationSerif-Regular.ttf");
pub const LIBERATION_SANS_DATA: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");
pub const LIBERATION_MONO_DATA: &[u8] = include_bytes!("../fonts/LiberationMono-Regular.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FamilyKind {
    Serif,
    Sans,
    Mono,
}

// adv[impl output.text-as-paths]
static SERIF: LazyLock<Face<'static>> = LazyLock::new(|| {
    Face::parse(LIBERATION_SERIF_DATA, 0).expect("LiberationSerif-Regular must parse")
});
static SANS: LazyLock<Face<'static>> = LazyLock::new(|| {
    Face::parse(LIBERATION_SANS_DATA, 0).expect("LiberationSans-Regular must parse")
});
static MONO: LazyLock<Face<'static>> = LazyLock::new(|| {
    Face::parse(LIBERATION_MONO_DATA, 0).expect("LiberationMono-Regular must parse")
});

fn face_for(kind: FamilyKind) -> &'static Face<'static> {
    match kind {
        FamilyKind::Serif => &SERIF,
        FamilyKind::Sans => &SANS,
        FamilyKind::Mono => &MONO,
    }
}

/// Look at a comma-separated `font-family` attribute value and pick
/// whether to render in Serif, Sans, or Mono. Case-insensitive,
/// respects the first recognised family. Unknown names fall through
/// to the next; empty / no hint defaults to Serif.
fn pick_family(font_family: &str) -> FamilyKind {
    for raw in font_family.split(',') {
        let token = raw
            .trim()
            .trim_matches(|c: char| c == '"' || c == '\'')
            .to_ascii_lowercase();
        match token.as_str() {
            // Monospace hints.
            "monospace" | "mono" | "courier" | "courier new" | "consolas" | "menlo"
            | "dejavu sans mono" | "liberation mono" | "jetbrains mono" | "fira code"
            | "source code pro" => {
                return FamilyKind::Mono;
            }
            // Sans hints.
            "sans-serif" | "sans" | "helvetica" | "helvetica neue" | "arial" | "verdana"
            | "geneva" | "tahoma" | "trebuchet" | "trebuchet ms" | "calibri" | "segoe ui"
            | "liberation sans" | "dejavu sans" | "noto sans" => {
                return FamilyKind::Sans;
            }
            // Serif hints.
            "serif" | "times" | "times new roman" | "georgia" | "cambria" | "garamond"
            | "palatino" | "liberation serif" | "dejavu serif" | "noto serif" | "stix two text" => {
                return FamilyKind::Serif;
            }
            _ => continue,
        }
    }
    FamilyKind::Serif
}

/// Replace every `<text …>content</text>` in `svg` with outlined
/// glyph paths followed by an invisible `<text>` for selection.
/// `<text>` elements we can't understand (unknown attributes, missing
/// glyphs across every bundled face) are left alone.
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
        let ch = svg[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn try_convert_text(bytes: &[u8]) -> Option<(usize, String)> {
    let open_close = bytes.iter().position(|&b| b == b'>')?;
    let tag_attrs = core::str::from_utf8(&bytes[5..open_close]).ok()?; // skip "<text"
    let after_open = open_close + 1;
    let content_end_rel = find_ci(&bytes[after_open..], b"</text>")?;
    let content = core::str::from_utf8(&bytes[after_open..after_open + content_end_rel]).ok()?;
    let total = after_open + content_end_rel + b"</text>".len();

    let x = parse_f64_attr(tag_attrs, "x")?;
    let y = parse_f64_attr(tag_attrs, "y")?;
    // font-size is usually present on graphviz output but pikchr
    // inherits it from the enclosing <svg>'s style. Fall back to a
    // sensible default matching pikchr's typical 14pt rendering.
    const DEFAULT_FONT_SIZE: f64 = 14.0;
    let font_size = parse_f64_attr(tag_attrs, "font-size").unwrap_or(DEFAULT_FONT_SIZE);
    let anchor = parse_str_attr(tag_attrs, "text-anchor").unwrap_or("start");
    let fill = parse_str_attr(tag_attrs, "fill").unwrap_or("currentColor");
    let family_hint = parse_str_attr(tag_attrs, "font-family").unwrap_or("");
    let primary = pick_family(family_hint);

    let content = unescape_xml(content);
    if content.is_empty() {
        return None;
    }

    // Walk each character, picking the primary face first and falling
    // through to the other two faces if the primary doesn't have a
    // glyph (so a stray `°` in a Sans label still renders instead of
    // dropping back to raw <text>). Bail only if NONE of the three
    // faces has the glyph.
    let mut glyphs: Vec<(&'static Face<'static>, GlyphId, f64, f64)> =
        Vec::with_capacity(content.chars().count());
    for ch in content.chars() {
        if ch.is_control() {
            continue;
        }
        let (face, glyph_id) = match find_glyph(ch, primary) {
            Some(p) => p,
            None => return None,
        };
        let upem = face.units_per_em();
        if upem == 0 {
            return None;
        }
        let scale = font_size / upem as f64;
        let advance = face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64 * scale;
        glyphs.push((face, glyph_id, scale, advance));
    }
    if glyphs.is_empty() {
        return None;
    }

    let total_advance: f64 = glyphs.iter().map(|(_, _, _, adv)| adv).sum();
    let start_x = match anchor {
        "middle" => x - total_advance / 2.0,
        "end" => x - total_advance,
        _ => x,
    };

    let mut out = String::with_capacity(256 + glyphs.len() * 80);
    let mut cursor_x = start_x;
    for (face, glyph_id, scale, advance) in &glyphs {
        if let Some(d) = extract_glyph_outline(face, *glyph_id) {
            out.push_str("<path d=\"");
            out.push_str(&d);
            out.push_str("\" transform=\"translate(");
            write_f64(&mut out, cursor_x);
            out.push(' ');
            write_f64(&mut out, y);
            out.push_str(") scale(");
            write_f64_precise(&mut out, *scale);
            out.push(' ');
            out.push('-');
            write_f64_precise(&mut out, *scale);
            out.push_str(")\" fill=\"");
            out.push_str(&xml_escape_attr(fill));
            out.push_str("\"/>");
        }
        cursor_x += advance;
    }
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

/// Find a glyph for `ch`, preferring the primary family but falling
/// through to the other two faces. Returns the face that actually
/// supplied the glyph plus the GlyphId within that face.
fn find_glyph(ch: char, primary: FamilyKind) -> Option<(&'static Face<'static>, GlyphId)> {
    let order = match primary {
        FamilyKind::Serif => [FamilyKind::Serif, FamilyKind::Sans, FamilyKind::Mono],
        FamilyKind::Sans => [FamilyKind::Sans, FamilyKind::Serif, FamilyKind::Mono],
        FamilyKind::Mono => [FamilyKind::Mono, FamilyKind::Sans, FamilyKind::Serif],
    };
    for kind in order {
        let face = face_for(kind);
        if let Some(g) = face.glyph_index(ch) {
            return Some((face, g));
        }
    }
    None
}

// Inline replacement for mathml_core::glyph::extract_glyph_path so
// this module doesn't pull a sibling-crate dep for 12 lines of TTF
// outline walking.
struct SvgPathBuilder {
    d: String,
}

impl OutlineBuilder for SvgPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "M{x} {y} ");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "L{x} {y} ");
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(self.d, "Q{x1} {y1} {x} {y} ");
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(self.d, "C{x1} {y1} {x2} {y2} {x} {y} ");
    }
    fn close(&mut self) {
        self.d.push_str("Z ");
    }
}

fn extract_glyph_outline(face: &Face<'_>, glyph_id: GlyphId) -> Option<String> {
    let mut b = SvgPathBuilder { d: String::new() };
    face.outline_glyph(glyph_id, &mut b);
    if b.d.is_empty() {
        None
    } else {
        Some(b.d.trim_end().to_owned())
    }
}

fn parse_f64_attr(attrs: &str, name: &str) -> Option<f64> {
    parse_str_attr(attrs, name).and_then(|v| v.trim().parse::<f64>().ok())
}

fn parse_str_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut search_from = 0;
    while let Some(pos) = attrs[search_from..].find(name) {
        let abs = search_from + pos;
        let preceded_by_ws =
            abs == 0 || matches!(attrs.as_bytes()[abs - 1], b' ' | b'\t' | b'\n' | b'\r');
        let after = abs + name.len();
        let followed_by_eq_ws = after < attrs.len()
            && (attrs.as_bytes()[after] == b'='
                || matches!(attrs.as_bytes()[after], b' ' | b'\t' | b'\n' | b'\r'));
        if preceded_by_ws && followed_by_eq_ws {
            let mut p = after;
            while p < attrs.len() && matches!(attrs.as_bytes()[p], b' ' | b'\t' | b'\n' | b'\r') {
                p += 1;
            }
            if p >= attrs.len() || attrs.as_bytes()[p] != b'=' {
                search_from = after;
                continue;
            }
            p += 1;
            while p < attrs.len() && matches!(attrs.as_bytes()[p], b' ' | b'\t' | b'\n' | b'\r') {
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
    let rounded = (v * 100.0).round() / 100.0;
    if rounded == rounded.trunc() {
        let _ = write!(out, "{}", rounded as i64);
    } else {
        let _ = write!(out, "{rounded:.2}");
    }
}

// Like `write_f64` but keeps 5 decimal places. Glyph scales are
// `font_size / upem` — for Liberation Sans/Serif (upem=2048) at
// font-size 14 that's ~0.00684, which `write_f64`'s 2-decimal rounding
// would emit as 0.01 (46% too large) or, at font-size 10, as 0 entirely.
fn write_f64_precise(out: &mut String, v: f64) {
    let rounded = (v * 100_000.0).round() / 100_000.0;
    if rounded == rounded.trunc() {
        let _ = write!(out, "{}", rounded as i64);
    } else {
        let s = format!("{rounded:.5}");
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        out.push_str(trimmed);
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
        // U+10400 Deseret capital Long I — absent from all three
        // Liberation faces, so the converter bails and the original
        // <text> is preserved verbatim so the browser's fallback
        // font can render it.
        let svg = r#"<text x="0" y="0" font-size="14">𐐀</text>"#;
        let out = text_to_paths(svg);
        assert!(out.contains("𐐀"), "got: {out}");
        assert!(!out.contains("<path d="), "got: {out}");
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn xml_entities_in_content_are_decoded_then_reescaped() {
        let svg = r#"<text x="0" y="0" font-size="14">a&amp;b</text>"#;
        let out = text_to_paths(svg);
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
        assert_eq!(out.matches("<text").count(), 2, "got: {out}");
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn pick_family_defaults_to_serif() {
        assert_eq!(pick_family(""), FamilyKind::Serif);
        assert_eq!(pick_family("Unknown,Foobar"), FamilyKind::Serif);
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn pick_family_respects_generic_keywords() {
        assert_eq!(pick_family("Times,serif"), FamilyKind::Serif);
        assert_eq!(pick_family("Helvetica,Arial,sans-serif"), FamilyKind::Sans);
        assert_eq!(pick_family("Courier,monospace"), FamilyKind::Mono);
    }

    // adv[verify output.text-as-paths]
    #[test]
    fn pick_family_case_insensitive() {
        assert_eq!(pick_family("HELVETICA"), FamilyKind::Sans);
        assert_eq!(pick_family("times new roman"), FamilyKind::Serif);
        assert_eq!(pick_family("COURIER NEW"), FamilyKind::Mono);
    }

    // adv[verify output.text-as-paths]
    // A space in Liberation Mono has a very different advance than in
    // Liberation Serif (mono is fixed-width, ~600 units; serif is
    // ~250). The translate() on the second <path> element tells us
    // which face was used for the first glyph.
    #[test]
    fn mono_family_produces_wider_advances_than_serif() {
        let mono_svg =
            r#"<text x="0" y="20" font-size="14" font-family="Courier,monospace">ii</text>"#;
        let serif_svg = r#"<text x="0" y="20" font-size="14" font-family="Times,serif">ii</text>"#;
        let mono_out = text_to_paths(mono_svg);
        let serif_out = text_to_paths(serif_svg);
        let mono_second_x = second_translate_x(&mono_out).expect("mono has second path");
        let serif_second_x = second_translate_x(&serif_out).expect("serif has second path");
        assert!(
            mono_second_x > serif_second_x + 1.0,
            "expected mono advance to exceed serif by >1 unit; mono={mono_second_x} serif={serif_second_x}"
        );
    }

    // adv[verify output.text-as-paths]
    // Regression: `write_f64`'s 2-decimal rounding was applied to the
    // glyph scale, so `font-size 14 / upem 2048 ≈ 0.00684` shipped as
    // `0.01` and glyphs rendered ~46% oversized vs. the cursor advance.
    // Pin this on font-size 12 where the bad value (0.01) is maximally
    // distinct from the correct value (~0.00586).
    #[test]
    fn glyph_scale_emitted_with_precision() {
        let svg = r#"<text x="0" y="20" font-size="12" font-family="Helvetica">A</text>"#;
        let out = text_to_paths(svg);
        let scale_str = first_scale_value(&out).expect("path has scale()");
        let parsed: f64 = scale_str.parse().expect("scale parses as f64");
        let upem = SANS.units_per_em() as f64;
        let expected = 12.0 / upem;
        let err = (parsed - expected).abs();
        assert!(
            err < 1e-4,
            "scale {parsed} differs from expected {expected} by {err} in: {out}",
        );
        assert!(
            (parsed - 0.01).abs() > 1e-4,
            "scale {parsed} is the buggy 0.01 value in: {out}",
        );
    }

    fn first_scale_value(svg: &str) -> Option<&str> {
        let idx = svg.find("scale(")? + "scale(".len();
        let end = svg[idx..].find(' ')? + idx;
        Some(&svg[idx..end])
    }

    fn second_translate_x(svg: &str) -> Option<f64> {
        let mut idx = svg.find("translate(")?;
        idx += "translate(".len();
        let _first_end = svg[idx..].find(')')? + idx;
        let rest = &svg[_first_end..];
        let next = rest.find("translate(")? + "translate(".len();
        let val_end = rest[next..].find(' ')? + next;
        rest[next..val_end].parse::<f64>().ok()
    }
}
