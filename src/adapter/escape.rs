//! Shared string-escaping helpers used by both the canonical vdom
//! renderer (`adapter::vdom`) when building raw-HTML node payloads and
//! the HTML serializer (`adapter::html`) when emitting text/attributes.

/// Percent-encode a URL for an `href`/`src`/`id` attribute, leaving the
/// URL-safe characters (and already-encoded `%XX`) untouched.
// [spec:mw:req:html.escape.url]
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
// [spec:mw:req:html.escape.text]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn href(url: &str) -> String {
        let mut out = String::new();
        escape_href(url, &mut out);
        out
    }

    // [spec:mw:req:html.escape.url/test]
    #[test]
    fn url_safe_characters_are_preserved_verbatim() {
        let preserved = "abcXYZ0189-._~:/?#@!$&'()*+,;=";
        assert_eq!(href(preserved), preserved);
    }

    // [spec:mw:req:html.escape.url/test]
    #[test]
    fn percent_is_preserved_so_encoded_sequences_are_not_double_encoded() {
        // %25XX would corrupt an already-working link.
        assert_eq!(href("/a%20b"), "/a%20b");
        assert_eq!(href("https://e.com/p?q=a%2Bb"), "https://e.com/p?q=a%2Bb");
        assert!(!href("/a%20b").contains("%2520"));
    }

    // [spec:mw:req:html.escape.url/test]
    #[test]
    fn attribute_and_tag_breaking_characters_are_percent_escaped() {
        // These are what stop a destination terminating the attribute or
        // opening a tag.
        assert_eq!(href("\""), "%22");
        assert_eq!(href("<"), "%3C");
        assert_eq!(href(">"), "%3E");
        assert_eq!(href(" "), "%20");
        assert_eq!(href("`"), "%60");
        assert_eq!(href("["), "%5B");
        assert_eq!(href("]"), "%5D");

        assert_eq!(
            href("/a\" onload=\"x"),
            "/a%22%20onload=%22x",
            "a quote must not survive into the attribute"
        );
    }

    // [spec:mw:req:html.escape.url/test]
    #[test]
    fn other_unlisted_ascii_and_all_non_ascii_are_escaped() {
        // Unlisted ASCII punctuation.
        assert_eq!(href("^"), "%5E");
        assert_eq!(href("{"), "%7B");
        assert_eq!(href("}"), "%7D");
        assert_eq!(href("|"), "%7C");
        assert_eq!(href("\\"), "%5C");
        // Control characters.
        assert_eq!(href("\n"), "%0A");
        assert_eq!(href("\t"), "%09");
        // Non-ASCII is UTF-8 encoded, then each byte escaped.
        assert_eq!(href("é"), "%C3%A9");
        assert_eq!(href("/ünïcode"), "/%C3%BCn%C3%AFcode");
        assert_eq!(href("→"), "%E2%86%92");
    }

    // [spec:mw:req:html.escape.url/test]
    #[test]
    fn urls_are_percent_encoded_not_html_escaped() {
        // HTML-escaping would emit `&amp;` and break the query string; the
        // percent-encoder must leave `&` and `=` alone.
        assert_eq!(href("/p?a=1&b=2"), "/p?a=1&b=2");
        let mut html = String::new();
        escape_html("/p?a=1&b=2", &mut html);
        assert_eq!(html, "/p?a=1&amp;b=2");
        assert_ne!(href("/p?a=1&b=2"), html);
    }
}
