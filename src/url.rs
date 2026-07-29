//! URL conventions used by markdawn for in-document navigation.
//!
//! Markdawn renders certain inline elements (footnote references) as
//! tappable links with custom URL schemes. Consumers wiring up a
//! `UITextViewDelegate` should call [`parse_markdawn_url`] inside
//! `textView:shouldInteractWith:in:interaction:` to decide whether to
//! intercept the tap and dispatch in-app navigation.
//!
//! ```ignore
//! impl UITextViewDelegate for MyDelegate {
//!     fn text_view_should_interact_with_url(&self, url: &NSURL, ...) -> bool {
//!         match markdawn::parse_markdawn_url(&url.absoluteString().to_string()) {
//!             Some(MarkdawnUrl::Footnote { label }) => {
//!                 self.scroll_to_footnote(&label);
//!                 false  // we handled it; don't open in browser
//!             }
//!             None => true,  // unknown scheme; let UIKit open externally
//!         }
//!     }
//! }
//! ```

/// In-document navigation target encoded in a markdawn URL.
// [spec:mw:def:core.url.scheme]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdawnUrl {
    /// A tap on a `[^label]` footnote reference. The `label` is the original
    /// (un-normalized) footnote label as it appeared in the source.
    Footnote { label: String },
}

/// Parse a URL string. Returns `Some` if the URL uses the `markdawn:` scheme
/// and matches a known navigation target; `None` otherwise (e.g., http/https
/// links that should open externally).
// [spec:mw:req:core.url.parse-strictness]
pub fn parse_markdawn_url(url: &str) -> Option<MarkdawnUrl> {
    let body = url.strip_prefix("markdawn:")?;
    if let Some(label) = body.strip_prefix("footnote/") {
        return Some(MarkdawnUrl::Footnote {
            label: label.to_string(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:def:core.url.scheme/test]
    #[test]
    fn parses_footnote_url() {
        assert_eq!(
            parse_markdawn_url("markdawn:footnote/note-1"),
            Some(MarkdawnUrl::Footnote {
                label: "note-1".to_string()
            })
        );
    }

    #[test]
    fn returns_none_for_http() {
        assert_eq!(parse_markdawn_url("https://example.com"), None);
    }

    // [spec:mw:req:core.url.parse-strictness/test]
    #[test]
    fn returns_none_for_unknown_markdawn_path() {
        assert_eq!(parse_markdawn_url("markdawn:wat"), None);
    }
}
