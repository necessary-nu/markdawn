//! Minimal `data:` URI parser.
//!
//! Format: `data:[<mediatype>][;base64],<data>`
//! - mediatype defaults to `text/plain;charset=US-ASCII` if omitted
//! - if `;base64` is present, the data is base64-encoded; otherwise it's
//!   percent-encoded

use base64::Engine;

// [spec:mw:def:uikit.media.data-uri]
pub struct DataUri<'a> {
    pub mime: &'a str,
    pub bytes: Vec<u8>,
}

// [spec:mw:def:uikit.media.data-uri]
pub fn parse_data_uri(uri: &str) -> Option<DataUri<'_>> {
    let rest = uri.strip_prefix("data:")?;
    let comma = rest.find(',')?;
    let (header, data) = rest.split_at(comma);
    let data = &data[1..]; // skip the comma

    let (mime, is_base64) = if let Some(stripped) = header.strip_suffix(";base64") {
        (stripped, true)
    } else {
        (header, false)
    };
    let mime = if mime.is_empty() {
        "text/plain;charset=US-ASCII"
    } else {
        mime
    };

    let bytes = if is_base64 {
        // Tolerate whitespace inside base64 (LLMs sometimes break long strings)
        let cleaned: String = data.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        base64::engine::general_purpose::STANDARD
            .decode(cleaned.as_bytes())
            .ok()?
    } else {
        percent_encoding::percent_decode_str(data).collect()
    };

    Some(DataUri { mime, bytes })
}

/// True when `mime` — a media type as returned by [`parse_data_uri`] —
/// names the SVG image type.
///
/// The header is matched as a media type rather than searched for a
/// substring: parameters after the first `;` are dropped and what
/// remains must equal `image/svg+xml`, or the shorter `image/svg` some
/// producers emit, case-insensitively. A substring test would accept
/// `text/html;charset=svg`, routing a payload that declares a scripting
/// media type into the SVG parser.
// [spec:mw:sem:uikit.media.image-source+1]
pub fn is_svg_media_type(mime: &str) -> bool {
    let base = match mime.split_once(';') {
        Some((base, _parameters)) => base,
        None => mime,
    };
    let base = base.trim_ascii();
    base.eq_ignore_ascii_case("image/svg+xml") || base.eq_ignore_ascii_case("image/svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:def:uikit.media.data-uri/test]
    #[test]
    fn parses_base64_svg() {
        let uri = "data:image/svg+xml;base64,PHN2Zy8+";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.mime, "image/svg+xml");
        assert_eq!(parsed.bytes, b"<svg/>");
    }

    // [spec:mw:def:uikit.media.data-uri/test]
    #[test]
    fn parses_percent_encoded() {
        let uri = "data:image/svg+xml,%3Csvg%2F%3E";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.mime, "image/svg+xml");
        assert_eq!(parsed.bytes, b"<svg/>");
    }

    // [spec:mw:def:uikit.media.data-uri/test]
    #[test]
    fn parses_with_no_mime() {
        let uri = "data:,Hello";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.mime, "text/plain;charset=US-ASCII");
        assert_eq!(parsed.bytes, b"Hello");
    }

    // [spec:mw:def:uikit.media.data-uri/test]
    #[test]
    fn rejects_non_data_uri() {
        assert!(parse_data_uri("https://example.com/foo.png").is_none());
        assert!(parse_data_uri("foo").is_none());
    }

    // [spec:mw:def:uikit.media.data-uri/test]
    #[test]
    fn rejects_data_with_no_comma() {
        assert!(parse_data_uri("data:image/png;base64").is_none());
    }

    // [spec:mw:def:uikit.media.data-uri/test]
    #[test]
    fn tolerates_whitespace_in_base64() {
        let uri = "data:image/svg+xml;base64,PHN2\nZy8\n+";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.bytes, b"<svg/>");
    }

    // [spec:mw:sem:uikit.media.image-source+1/test]
    #[test]
    fn svg_media_type_accepts_the_svg_image_types() {
        assert!(is_svg_media_type("image/svg+xml"));
        assert!(is_svg_media_type("IMAGE/SVG+XML"));
        assert!(is_svg_media_type("image/svg"));
        assert!(is_svg_media_type("image/svg+xml;charset=utf-8"));
        assert!(is_svg_media_type(" image/svg+xml "));
    }

    /// The predicate must not be a substring test: a header carrying
    /// `svg` in its subtype tail or in a parameter still declares a
    /// media type that is not SVG, and its payload must not reach the
    /// SVG parser.
    // [spec:mw:sem:uikit.media.image-source+1/test]
    #[test]
    fn svg_media_type_rejects_svg_as_a_substring() {
        assert!(!is_svg_media_type("text/html;charset=svg"));
        assert!(!is_svg_media_type("text/html;svg"));
        assert!(!is_svg_media_type("image/png;name=logo.svg"));
        assert!(!is_svg_media_type("application/svg+xml"));
        assert!(!is_svg_media_type("image/svg+xml-ish"));
        assert!(!is_svg_media_type("text/plain;charset=US-ASCII"));
    }

    /// The media type of a header with no `;base64` suffix still parses
    /// out of the parameters the parser returns verbatim.
    // [spec:mw:sem:uikit.media.image-source+1/test]
    #[test]
    fn parsed_header_feeds_the_media_type_predicate() {
        let hostile = parse_data_uri("data:text/html;charset=svg,%3Csvg%3E%3C%2Fsvg%3E").unwrap();
        assert_eq!(hostile.mime, "text/html;charset=svg");
        assert!(!is_svg_media_type(hostile.mime));

        let genuine = parse_data_uri("data:image/svg+xml;base64,PHN2Zy8+").unwrap();
        assert!(is_svg_media_type(genuine.mime));
    }
}
