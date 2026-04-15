//! Minimal `data:` URI parser.
//!
//! Format: `data:[<mediatype>][;base64],<data>`
//! - mediatype defaults to `text/plain;charset=US-ASCII` if omitted
//! - if `;base64` is present, the data is base64-encoded; otherwise it's
//!   percent-encoded

use base64::Engine;

pub struct DataUri<'a> {
    pub mime: &'a str,
    pub bytes: Vec<u8>,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_base64_svg() {
        let uri = "data:image/svg+xml;base64,PHN2Zy8+";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.mime, "image/svg+xml");
        assert_eq!(parsed.bytes, b"<svg/>");
    }

    #[test]
    fn parses_percent_encoded() {
        let uri = "data:image/svg+xml,%3Csvg%2F%3E";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.mime, "image/svg+xml");
        assert_eq!(parsed.bytes, b"<svg/>");
    }

    #[test]
    fn parses_with_no_mime() {
        let uri = "data:,Hello";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.mime, "text/plain;charset=US-ASCII");
        assert_eq!(parsed.bytes, b"Hello");
    }

    #[test]
    fn rejects_non_data_uri() {
        assert!(parse_data_uri("https://example.com/foo.png").is_none());
        assert!(parse_data_uri("foo").is_none());
    }

    #[test]
    fn rejects_data_with_no_comma() {
        assert!(parse_data_uri("data:image/png;base64").is_none());
    }

    #[test]
    fn tolerates_whitespace_in_base64() {
        let uri = "data:image/svg+xml;base64,PHN2\nZy8\n+";
        let parsed = parse_data_uri(uri).unwrap();
        assert_eq!(parsed.bytes, b"<svg/>");
    }
}
