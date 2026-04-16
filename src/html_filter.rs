//! Configurable HTML tag filtering for adapters.
//!
//! Defaults to a safe-by-default policy that neutralizes the GFM
//! disallowed-raw-html-extension tag list (`script`, `iframe`, `style`,
//! etc.) by replacing the leading `<` with `&lt;` so the content renders
//! as text instead of executing.
//!
//! Adapters apply this filter to every `Node::Html` they render, regardless
//! of whether GFM was enabled at parse time. Consumers who need to allow
//! arbitrary HTML can call [`HtmlFilter::unrestricted`].

/// The GFM tagfilter list. Replacing `<` with `&lt;` for these tag names
/// (case-insensitive) is the standard "safe HTML in markdown" baseline.
pub const SAFE_DISALLOWED_TAGS: &[&str] = &[
    "textarea",
    "title",
    "style",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "script",
    "plaintext",
];

/// Configurable HTML tag filter. Use [`HtmlFilter::safe`] for the
/// GFM-style disallowed-tag list (the default), or
/// [`HtmlFilter::unrestricted`] to pass everything through untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlFilter {
    disallowed: Vec<String>,
}

impl Default for HtmlFilter {
    fn default() -> Self {
        Self::safe()
    }
}

impl HtmlFilter {
    /// Filter using the GFM tagfilter list.
    pub fn safe() -> Self {
        Self {
            disallowed: SAFE_DISALLOWED_TAGS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }

    /// Pass all HTML through unchanged. Use only when you trust the source.
    pub fn unrestricted() -> Self {
        Self {
            disallowed: Vec::new(),
        }
    }

    /// Build a filter with a custom list of tag names to neutralize.
    /// Tag matching is case-insensitive.
    pub fn with_disallowed_tags<I, S>(tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            disallowed: tags.into_iter().map(Into::into).collect(),
        }
    }

    /// Add an additional tag name to the disallowed list.
    pub fn deny<S: Into<String>>(mut self, tag: S) -> Self {
        self.disallowed.push(tag.into());
        self
    }

    /// True if no tags are filtered (i.e. [`HtmlFilter::unrestricted`]).
    pub fn is_unrestricted(&self) -> bool {
        self.disallowed.is_empty()
    }

    /// Apply the filter, returning the (possibly modified) HTML string.
    /// Replaces the `<` that opens any disallowed tag with `&lt;` so the
    /// tag renders as visible text instead of executing.
    pub fn filter(&self, html: &str) -> String {
        if self.disallowed.is_empty() {
            return html.to_string();
        }
        let mut result = String::with_capacity(html.len());
        let mut i = 0;
        let bytes = html.as_bytes();

        while i < bytes.len() {
            if bytes[i] == b'<' {
                let after_angle = i + 1;
                let is_closing = after_angle < bytes.len() && bytes[after_angle] == b'/';
                let tag_name_start = if is_closing {
                    after_angle + 1
                } else {
                    after_angle
                };

                let mut matched = false;
                for tag in &self.disallowed {
                    let tag_end = tag_name_start + tag.len();
                    if tag_end <= bytes.len() {
                        let candidate = &html[tag_name_start..tag_end];
                        if candidate.eq_ignore_ascii_case(tag) {
                            let boundary_ok = tag_end >= bytes.len()
                                || matches!(bytes[tag_end], b' ' | b'\t' | b'\n' | b'>' | b'/');
                            if boundary_ok {
                                result.push_str("&lt;");
                                i += 1;
                                matched = true;
                                break;
                            }
                        }
                    }
                }

                if !matched {
                    result.push('<');
                    i += 1;
                }
            } else {
                let ch = html[i..].chars().next().unwrap();
                result.push(ch);
                i += ch.len_utf8();
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_neutralizes_script() {
        let filtered = HtmlFilter::safe().filter("<script>alert(1)</script>");
        assert_eq!(filtered, "&lt;script>alert(1)&lt;/script>");
    }

    #[test]
    fn safe_passes_through_div() {
        let filtered = HtmlFilter::safe().filter("<div>hello</div>");
        assert_eq!(filtered, "<div>hello</div>");
    }

    #[test]
    fn unrestricted_passes_script_unchanged() {
        let filtered = HtmlFilter::unrestricted().filter("<script>foo</script>");
        assert_eq!(filtered, "<script>foo</script>");
    }

    #[test]
    fn custom_disallowed_tag() {
        let filter = HtmlFilter::safe().deny("custom-element");
        let filtered = filter.filter("<custom-element>x</custom-element>");
        assert_eq!(filtered, "&lt;custom-element>x&lt;/custom-element>");
    }

    #[test]
    fn case_insensitive_match() {
        let filtered = HtmlFilter::safe().filter("<SCRIPT>x</SCRIPT>");
        assert_eq!(filtered, "&lt;SCRIPT>x&lt;/SCRIPT>");
    }
}
