//! Scheme allow-list for URLs that get emitted into generated HTML / SVG.
//!
//! Used by every adapter path that takes a user-supplied URL (markdown
//! `[text](URL)`, `![alt](URL)`, autolinks) and by the SVG sanitiser
//! (`href`, `xlink:href` attributes in graphviz / pikchr output). The
//! goal is to reject URLs that browsers would execute as script —
//! `javascript:`, `data:`, `vbscript:`, `file:`, and anything with a
//! scheme we don't recognise.

const ALLOWED_SCHEMES: &[&str] = &[
    "http", "https", "mailto", "tel", "sms", "ftp", "ftps", "markdawn",
];

/// Returns true if `url` is safe to emit as a link / image / href /
/// xlink:href destination.
///
/// Accepts:
/// - URLs with a scheme in [`ALLOWED_SCHEMES`] (case-insensitive).
/// - Fragment-only URLs (`#section`).
/// - Absolute paths (`/foo/bar`).
/// - Relative URLs with no scheme at all (`foo.html`, `../bar`).
///
/// Rejects:
/// - Any URL containing ASCII control characters (0x00–0x1F, 0x7F) —
///   browsers strip whitespace / NULs in scheme parsing, so
///   `jav\tascript:` or `java\0script:` becomes `javascript:`. We
///   reject the whole input rather than try to second-guess normalisation.
/// - URLs with a scheme not in the allow-list: `javascript:`, `data:`,
///   `vbscript:`, `file:`, `blob:`, etc.
// adv[impl markdown.url-allowlist]
pub fn is_safe_url(url: &str) -> bool {
    // Reject any control chars (including TAB and NL). Browsers strip
    // these during scheme parsing, so allowing them would let an
    // attacker smuggle `javascript:` past a naive scheme comparison.
    if url.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return false;
    }

    let trimmed = url.trim_start();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.starts_with('#') || trimmed.starts_with('/') {
        return true;
    }

    // RFC 3986 scheme grammar: ALPHA *(ALPHA / DIGIT / "+" / "-" / ".")
    // followed by ":". If we don't find a colon that terminates a valid
    // scheme, treat as a relative reference.
    let Some(colon) = trimmed.find(':') else {
        return true;
    };
    let scheme = &trimmed[..colon];
    if scheme.is_empty() {
        // ":foo" — not a real scheme, treat as relative.
        return true;
    }
    let mut chars = scheme.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() {
        return true;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return true;
    }

    let lower = scheme.to_ascii_lowercase();
    ALLOWED_SCHEMES.iter().any(|s| *s == lower)
}

/// Like [`is_safe_url`] but also accepts `data:image/<mime>;...` URIs.
///
/// Browsers do not execute scripts in SVG (or any other image format)
/// loaded via `<img>` — that's a documented sandbox boundary in the
/// HTML spec — so a `data:image/svg+xml` URL emitted into an `<img
/// src>` cannot run JS the way `data:text/html` could. Restricting
/// the relaxation to image MIME types keeps that property: a
/// `data:text/html;…` URI still gets dropped.
// adv[impl markdown.url-allowlist]
pub fn is_safe_image_url(url: &str) -> bool {
    if is_safe_url(url) {
        return true;
    }
    if url.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return false;
    }
    let trimmed = url.trim_start();
    let prefix = "data:image/";
    trimmed.len() >= prefix.len()
        && trimmed.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_and_https_accepted() {
        assert!(is_safe_url("http://example.com"));
        assert!(is_safe_url("https://example.com/a/b?c=d#e"));
    }

    #[test]
    fn mailto_tel_sms_ftp_accepted() {
        assert!(is_safe_url("mailto:foo@example.com"));
        assert!(is_safe_url("tel:+15551234567"));
        assert!(is_safe_url("sms:+15551234567"));
        assert!(is_safe_url("ftp://example.com/file"));
    }

    #[test]
    fn markdawn_internal_scheme_accepted() {
        assert!(is_safe_url("markdawn://footnote/1"));
    }

    #[test]
    fn fragment_accepted() {
        assert!(is_safe_url("#section"));
    }

    #[test]
    fn relative_and_absolute_paths_accepted() {
        assert!(is_safe_url("/abs/path"));
        assert!(is_safe_url("rel/path"));
        assert!(is_safe_url("../up"));
        assert!(is_safe_url(""));
    }

    #[test]
    fn javascript_scheme_rejected() {
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("JavaScript:alert(1)"));
        assert!(!is_safe_url("JAVASCRIPT:alert(1)"));
    }

    #[test]
    fn data_vbscript_file_blob_rejected() {
        assert!(!is_safe_url("data:text/html,<script>alert(1)</script>"));
        assert!(!is_safe_url("vbscript:msgbox(1)"));
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("blob:http://example.com/xyz"));
    }

    #[test]
    fn control_chars_smuggling_rejected() {
        assert!(!is_safe_url("java\tscript:alert(1)"));
        assert!(!is_safe_url("javascript\0:alert(1)"));
        assert!(!is_safe_url("javascript:\x0Aalert(1)"));
        assert!(!is_safe_url("http://example\n.com"));
        assert!(!is_safe_url("http://example\r\n.com"));
    }

    #[test]
    fn leading_whitespace_does_not_promote_javascript() {
        // The leading space is trimmed for the scheme lookup, but even
        // if it weren't, the scheme "javascript" isn't allow-listed.
        assert!(!is_safe_url(" javascript:alert(1)"));
        assert!(!is_safe_url("   javascript:alert(1)"));
    }

    #[test]
    fn colon_in_relative_path_is_not_a_scheme() {
        // "C:foo" has a single-char "scheme" that IS valid per RFC 3986,
        // but "c" isn't allow-listed so we reject it. That's the
        // conservative call — breaks the unusual Windows-path case but
        // catches "x:javascript" smuggling.
        assert!(!is_safe_url("c:foo/bar"));
        // "1abc:foo" — scheme starts with digit, not a real scheme,
        // treat as relative.
        assert!(is_safe_url("1abc:foo"));
    }
}
