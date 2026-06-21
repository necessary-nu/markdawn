//! Shared string-escaping helpers used by both the canonical vdom
//! renderer (`adapter::vdom`) when building raw-HTML node payloads and
//! the HTML serializer (`adapter::html`) when emitting text/attributes.

/// Percent-encode a URL for an `href`/`src`/`id` attribute, leaving the
/// URL-safe characters (and already-encoded `%XX`) untouched.
pub(crate) fn escape_href(url: &str, out: &mut String) {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

    const URL_SAFE: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~')
        .remove(b':')
        .remove(b'/')
        .remove(b'?')
        .remove(b'#')
        .remove(b'@')
        .remove(b'!')
        .remove(b'$')
        .remove(b'&')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')')
        .remove(b'*')
        .remove(b'+')
        .remove(b',')
        .remove(b';')
        .remove(b'=')
        .remove(b'%');

    out.push_str(&utf8_percent_encode(url, URL_SAFE).to_string());
}

/// HTML-escape text for a text node or a quoted attribute value.
pub(crate) fn escape_html(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}
