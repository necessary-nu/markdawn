//! Renderer-neutral syntax-highlighting boundary.
//!
//! Markdawn owns the public span and error types consumed by its renderers.
//! The Arborium implementation below is a transition adapter: it keeps the
//! current feature working without making Arborium types part of renderer APIs.

use std::fmt;

#[cfg(feature = "highlight")]
use arborium::advanced::HighlightSpans;

/// A semantic syntax span over UTF-8 source bytes.
///
/// Ranges are half-open (`start..end`). Spans may overlap or nest, and
/// `pattern_index` retains query priority when equivalent ranges compete.
// [spec:mw:req:highlighting.span-semantics]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxSpan {
    /// Inclusive UTF-8 byte offset.
    pub start: u32,
    /// Exclusive UTF-8 byte offset.
    pub end: u32,
    /// Raw capture name emitted by the highlighting provider.
    pub capture: String,
    /// Query pattern index; higher indices have higher priority.
    pub pattern_index: u32,
}

/// Provider diagnostic returned by [`SyntaxHighlighter`].
///
/// Renderers may fall back to plain output, but the highlighter boundary keeps
/// the diagnostic available to callers instead of turning failure into
/// absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightError {
    message: String,
}

impl HighlightError {
    /// Create a provider-neutral highlighting diagnostic.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The provider diagnostic text.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for HighlightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HighlightError {}

/// Produces renderer-neutral semantic spans for source code.
// [spec:mw:req:highlighting.api]
pub trait SyntaxHighlighter {
    /// Highlight `source` using the supplied language hint.
    fn highlight(
        &mut self,
        language_hint: &str,
        source: &str,
    ) -> Result<Vec<SyntaxSpan>, HighlightError>;
}

/// Temporary compatibility adapter for the legacy Arborium feature.
///
/// The blanket implementation intentionally terminates at Markdawn-owned
/// types. Renderers never receive Arborium spans or errors.
// [spec:mw:req:highlighting.arborium-transition]
#[cfg(feature = "highlight")]
impl<H> SyntaxHighlighter for H
where
    H: HighlightSpans,
    H::Error: fmt::Display,
{
    fn highlight(
        &mut self,
        language_hint: &str,
        source: &str,
    ) -> Result<Vec<SyntaxSpan>, HighlightError> {
        self.highlight_spans(language_hint, source)
            .map(|spans| spans.into_iter().map(SyntaxSpan::from).collect())
            .map_err(|error| HighlightError::new(error.to_string()))
    }
}

#[cfg(feature = "highlight")]
impl From<arborium::advanced::Span> for SyntaxSpan {
    fn from(span: arborium::advanced::Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
            capture: span.capture,
            pattern_index: span.pattern_index,
        }
    }
}

#[cfg(feature = "highlight")]
impl From<SyntaxSpan> for arborium::advanced::Span {
    fn from(span: SyntaxSpan) -> Self {
        Self {
            start: span.start,
            end: span.end,
            capture: span.capture,
            pattern_index: span.pattern_index,
        }
    }
}

/// Render neutral spans with the legacy HTML formatter during the transition.
#[cfg(feature = "highlight")]
pub(crate) fn spans_to_html(source: &str, spans: Vec<SyntaxSpan>) -> String {
    arborium::advanced::spans_to_html(
        source,
        spans.into_iter().map(Into::into).collect(),
        &arborium::advanced::HtmlFormat::default(),
    )
}

#[cfg(all(test, feature = "highlight"))]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct StubError(&'static str);

    impl fmt::Display for StubError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    struct StubArborium {
        result: Result<Vec<arborium::advanced::Span>, StubError>,
    }

    impl HighlightSpans for StubArborium {
        type Error = StubError;

        fn highlight_spans(
            &mut self,
            _language: &str,
            _source: &str,
        ) -> Result<Vec<arborium::advanced::Span>, Self::Error> {
            std::mem::replace(&mut self.result, Ok(Vec::new()))
        }
    }

    // [spec:mw:req:highlighting.api/test]
    // [spec:mw:req:highlighting.span-semantics/test]
    // [spec:mw:req:highlighting.arborium-transition/test]
    #[test]
    fn arborium_adapter_preserves_neutral_span_contract() {
        let expected = vec![
            SyntaxSpan {
                start: 0,
                end: 6,
                capture: "function.call".into(),
                pattern_index: 2,
            },
            SyntaxSpan {
                start: 2,
                end: 6,
                capture: "variable.parameter".into(),
                pattern_index: 9,
            },
        ];
        let mut highlighter = StubArborium {
            result: Ok(expected.clone().into_iter().map(Into::into).collect()),
        };

        let actual = highlighter.highlight("rust", "éname").unwrap();

        assert_eq!(actual, expected);
    }

    // [spec:mw:req:highlighting.error-fallback/test]
    // [spec:mw:req:highlighting.arborium-transition/test]
    #[test]
    fn arborium_adapter_preserves_provider_diagnostic() {
        let mut highlighter = StubArborium {
            result: Err(StubError("query compilation failed")),
        };

        let error = highlighter.highlight("rust", "fn main() {}").unwrap_err();

        assert_eq!(error.message(), "query compilation failed");
    }

    // [spec:mw:req:highlighting.span-semantics/test]
    #[test]
    fn html_transition_respects_byte_ranges_and_pattern_priority() {
        let spans = vec![
            SyntaxSpan {
                start: 2,
                end: 6,
                capture: "keyword".into(),
                pattern_index: 1,
            },
            SyntaxSpan {
                start: 2,
                end: 6,
                capture: "string".into(),
                pattern_index: 7,
            },
        ];

        assert_eq!(spans_to_html("éname", spans), "é<a-s>name</a-s>");
    }
}
