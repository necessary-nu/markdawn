//! Defence-in-depth sanitiser for SVG blobs we emit into HTML.
//!
//! Every adapter path that produces or accepts attacker-influenced SVG
//! (graphviz output, pikchr output, future sources) runs its bytes
//! through [`sanitize_svg`] before they land in the rendered page via
//! `dangerous_inner_html` or a raw string concat.
//!
//! The sanitiser targets the exact threats that have produced CVEs in
//! graphviz / pikchr downstream:
//! - `<script>…</script>` blocks (dropped entirely).
//! - `on*="…"` event-handler attributes (attribute dropped).
//! - `href` / `xlink:href` attributes whose URL scheme isn't on the
//!   [`crate::url_safety`] allow-list — value replaced with `"#"`.
//!
//! This is a single-pass byte scanner, not an XML parser. It assumes
//! the input is the kind of SVG graphviz or pikchr actually produces
//! (simple attribute quoting with `"`, no CDATA, no entities in attr
//! values). The scanner errs on the side of dropping dangerous bytes
//! when the shape is unexpected — false positives cost a rendered
//! link; false negatives cost XSS.

use crate::url_safety::is_safe_url;

/// Maximum byte length of a rendered SVG we'll pass back to the
/// caller. Graphviz can turn a small DOT input into a massive SVG
/// (long labels on many edges, etc.); cap the amplification.
pub const MAX_SVG_BYTES: usize = 5 * 1024 * 1024;

/// Sanitise `svg` in place. Returns the sanitised string. Never fails;
/// the output is always a valid byte string (may be empty if input was
/// dangerous enough to strip everything).
pub fn sanitize_svg(svg: &str) -> String {
    let bytes = svg.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // <script> blocks — match case-insensitively, skip to </script>.
        if b == b'<' && starts_with_ci(&bytes[i..], b"<script") {
            // Find the matching </script> (case-insensitive). If we
            // can't find one, drop the rest of the input to stay safe.
            if let Some(end) = find_ci(&bytes[i..], b"</script>") {
                i += end + b"</script>".len();
            } else {
                // Unterminated; drop the rest.
                break;
            }
            continue;
        }

        // Attribute start: inside a tag, an attribute looks like
        // `\s+name="value"`. We only look at attribute starts after a
        // `<` has been seen — simpler approach: scan for the specific
        // attribute-name patterns that are dangerous, anywhere. The
        // byte preceding the name must be whitespace (so we don't
        // match "fxhref=" as "href=").
        if is_attr_boundary(bytes, i) {
            // on<letter>+="..." → drop attribute.
            if bytes[i] == b'o' && is_event_handler_start(&bytes[i..]) {
                if let Some(skipped) = skip_attribute(&bytes[i..]) {
                    i += skipped;
                    continue;
                }
            }
            // xlink:href="..." / href="..." → validate scheme.
            if let Some((attr_end, value_range)) = match_href_attr(&bytes[i..]) {
                let scheme_ok = core::str::from_utf8(&bytes[i + value_range.0..i + value_range.1])
                    .map(is_safe_url)
                    .unwrap_or(false);
                if scheme_ok {
                    // Copy the attribute as-is.
                    out.push_str(core::str::from_utf8(&bytes[i..i + attr_end]).unwrap_or(""));
                } else {
                    // Copy `name="#"` — preserve the attribute name so
                    // SVG stays well-formed, but neuter the value.
                    let name_end = value_range.0 - 1; // strip the opening quote
                    out.push_str(core::str::from_utf8(&bytes[i..i + name_end]).unwrap_or(""));
                    out.push_str("\"#\"");
                }
                i += attr_end;
                continue;
            }
        }

        // Default: copy the byte and advance.
        out.push(b as char);
        i += 1;
    }

    out
}

fn is_attr_boundary(bytes: &[u8], i: usize) -> bool {
    // An attribute name starts where the preceding byte is whitespace
    // and we're inside something that looks like a tag. Rather than
    // track tag state, we use the weaker rule: preceded by whitespace.
    i > 0 && matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'\r')
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

/// Check if `bytes[..]` begins with an event-handler attribute name
/// (`on` + one or more ASCII alpha chars + `=`).
fn is_event_handler_start(bytes: &[u8]) -> bool {
    if bytes.len() < 3 {
        return false;
    }
    if !(bytes[0] == b'o' && bytes[1] == b'n') {
        return false;
    }
    let mut j = 2;
    while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
        j += 1;
    }
    j > 2 && j < bytes.len() && bytes[j] == b'='
}

/// Match a full attribute (name through closing quote of value) and
/// return the total length consumed. Bail out on malformed input.
fn skip_attribute(bytes: &[u8]) -> Option<usize> {
    // Consume name up to `=`.
    let eq = bytes.iter().position(|&b| b == b'=')?;
    let after_eq = eq + 1;
    if after_eq >= bytes.len() {
        return None;
    }
    let quote = bytes[after_eq];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let value_start = after_eq + 1;
    let close = bytes[value_start..].iter().position(|&b| b == quote)?;
    Some(value_start + close + 1)
}

/// If `bytes[..]` begins with `href="..."` or `xlink:href="..."`,
/// return `(total_length, (value_start_rel_to_bytes, value_end_rel))`.
fn match_href_attr(bytes: &[u8]) -> Option<(usize, (usize, usize))> {
    let name_len = if starts_with_ci(bytes, b"xlink:href=") {
        "xlink:href=".len()
    } else if starts_with_ci(bytes, b"href=") {
        "href=".len()
    } else {
        return None;
    };
    if name_len >= bytes.len() {
        return None;
    }
    let quote = bytes[name_len];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let value_start = name_len + 1;
    let close = bytes[value_start..].iter().position(|&b| b == quote)?;
    let value_end = value_start + close;
    let total = value_end + 1;
    Some((total, (value_start, value_end)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benign_svg_passes_unchanged() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><text>hello</text></g></svg>"#;
        assert_eq!(sanitize_svg(svg), svg);
    }

    #[test]
    fn graphviz_shaped_safe_href_survives() {
        let svg = r#"<g><a xlink:href="https://example.com" xlink:title="hi"><ellipse/></a></g>"#;
        let out = sanitize_svg(svg);
        assert!(out.contains(r#"xlink:href="https://example.com""#), "got: {out}");
    }

    #[test]
    fn javascript_xlink_href_is_neutered() {
        let svg = r#"<g><a xlink:href="javascript:alert(1)"><ellipse/></a></g>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("javascript:"), "got: {out}");
        assert!(out.contains("xlink:href=\"#\""), "got: {out}");
    }

    #[test]
    fn javascript_href_plain_is_neutered() {
        let svg = r#"<a href="javascript:alert(1)">x</a>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("javascript:"), "got: {out}");
        assert!(out.contains("href=\"#\""));
    }

    #[test]
    fn data_url_in_href_rejected() {
        let svg = r#"<a href="data:text/html,<script>alert(1)</script>">x</a>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("data:"), "got: {out}");
    }

    #[test]
    fn script_block_dropped() {
        let svg = r#"<svg><script>alert(1)</script><text>x</text></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("<script"), "got: {out}");
        assert!(!out.contains("alert"), "got: {out}");
        assert!(out.contains("<text>"));
    }

    #[test]
    fn script_block_case_insensitive() {
        let svg = r#"<svg><SCRIPT>alert(1)</SCRIPT></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("alert"), "got: {out}");
    }

    #[test]
    fn event_handler_attribute_dropped() {
        let svg = r#"<ellipse onmouseover="alert(1)" cx="0" cy="0"/>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("onmouseover"), "got: {out}");
        assert!(out.contains("cx=\"0\""), "got: {out}");
    }

    #[test]
    fn onload_handler_dropped_from_svg_root() {
        let svg = r#"<svg onload="alert(1)" xmlns="http://www.w3.org/2000/svg"></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("onload"), "got: {out}");
    }

    #[test]
    fn unterminated_script_truncates() {
        let svg = r#"<svg><script>alert(1)"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("alert"), "got: {out}");
    }

    #[test]
    fn href_with_relative_url_survives() {
        let svg = r#"<a xlink:href="../foo.html">x</a>"#;
        let out = sanitize_svg(svg);
        assert!(out.contains(r#"xlink:href="../foo.html""#), "got: {out}");
    }

    #[test]
    fn href_with_fragment_survives() {
        let svg = r##"<a xlink:href="#section">x</a>"##;
        let out = sanitize_svg(svg);
        assert!(out.contains(r##"xlink:href="#section""##), "got: {out}");
    }
}
