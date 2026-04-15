//! Bridge between arborium's `HighlightSpans` trait (with generic `Error`) and
//! the adapters, which need a dyn-compatible object.
//!
//! The `SpanHighlighter` trait here erases the concrete error type by turning
//! any failure into `None`. Callers that need detailed error info should invoke
//! the underlying `HighlightSpans::highlight_spans` directly.

use arborium::advanced::{HighlightSpans, Span};

/// Object-safe wrapper around arborium's `HighlightSpans`: any failure collapses
/// to `None`, letting adapters fall back to plain rendering.
pub trait SpanHighlighter {
    fn spans(&mut self, lang: &str, source: &str) -> Option<Vec<Span>>;
}

impl<H: HighlightSpans> SpanHighlighter for H {
    fn spans(&mut self, lang: &str, source: &str) -> Option<Vec<Span>> {
        self.highlight_spans(lang, source).ok()
    }
}
