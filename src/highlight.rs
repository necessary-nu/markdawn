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
/// types. Renderers never receive Arborium spans or errors. Deliberately
/// unannotated: `highlighting.provider-binding` also forbids re-exporting the
/// backend crate, which `src/lib.rs` still does, so claiming that rule here
/// would be a false claim. `ntreesitter.core-binding` replaces this adapter
/// and closes the rule.
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

// The span algebra below is platform-neutral arithmetic over `SyntaxSpan`,
// but its only caller today is the UIKit adapter, which compiles solely
// for `target_os = "ios"` — hence the `dead_code` waivers on the entry
// points. Living here rather than inside that adapter is what makes the
// logic testable on a non-Apple host.

/// The palette slot a capture name resolves to, independent of any
/// toolkit's colour type.
///
/// Resolving a capture name and looking up a colour are separate
/// concerns: the walk from `keyword.function` to `keyword` is the same
/// everywhere, while the colour itself is a platform type (`UIColor` on
/// UIKit, an RGB triple in iced). Renderers resolve to an accent here
/// and map the accent onto their own colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyntaxAccent {
    /// Keywords, imports, storage classes, control flow.
    Keyword,
    /// Function, method and macro names.
    Function,
    /// Type, namespace, module and class names.
    Type,
    /// String and character literals.
    StringLiteral,
    /// Comments.
    Comment,
    /// Numeric, boolean and named constants.
    Constant,
    /// Attributes, markup tags and properties.
    Attribute,
    /// Escape sequences inside literals.
    Escape,
    /// Labels.
    Label,
}

/// Resolve a raw capture name (e.g. `"keyword.function"`) to an accent,
/// retrying against successively shorter dotted prefixes. `None` when
/// the name is unmapped at every level, or when it names a capture that
/// is deliberately unaccented.
// [spec:mw:req:uikit.syntax.capture-fallback]
#[allow(dead_code)]
pub(crate) fn capture_accent(capture: &str) -> Option<SyntaxAccent> {
    let mut name = capture;
    loop {
        if let Some(accent) = exact_capture_accent(name) {
            return Some(accent);
        }
        let dot = name.rfind('.')?;
        name = &name[..dot];
    }
}

/// The accent table, keyed on a whole capture name. Consulted by
/// [`capture_accent`] once per dotted prefix.
// [spec:mw:req:uikit.syntax.capture-fallback]
fn exact_capture_accent(capture: &str) -> Option<SyntaxAccent> {
    let accent = match capture {
        "keyword" | "include" | "storageclass" | "conditional" | "repeat" => SyntaxAccent::Keyword,
        "function" | "function.builtin" | "function.macro" | "method" => SyntaxAccent::Function,
        "type" | "type.builtin" | "namespace" | "module" | "class" => SyntaxAccent::Type,
        "string" | "string.special" | "character" => SyntaxAccent::StringLiteral,
        "comment" => SyntaxAccent::Comment,
        "constant" | "constant.builtin" | "boolean" | "number" | "float" => SyntaxAccent::Constant,
        // Deliberately unaccented: syntax that reads better in the base
        // text colour than in an accent of its own. Named rather than
        // left to the catch-all so the intent survives editing.
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => {
            return None;
        }
        "variable" | "variable.parameter" | "variable.builtin" => return None,
        "attribute" | "tag" | "tag.attribute" | "property" => SyntaxAccent::Attribute,
        "escape" | "string.escape" => SyntaxAccent::Escape,
        "label" => SyntaxAccent::Label,
        _ => return None,
    };
    Some(accent)
}

/// Order `spans` for painting: widest first, ties broken by ascending
/// start offset and then ascending `pattern_index`.
///
/// Painting in this order means an enclosing capture is painted before
/// the captures nested inside it, and — for equal ranges — the
/// higher-priority query pattern is painted last and wins.
// [spec:mw:sem:uikit.syntax.span-priority]
#[allow(dead_code)]
pub(crate) fn order_spans(spans: &[SyntaxSpan]) -> Vec<&SyntaxSpan> {
    let mut ordered: Vec<&SyntaxSpan> = spans.iter().collect();
    ordered.sort_by(|a, b| {
        let a_width = a.end.saturating_sub(a.start);
        let b_width = b.end.saturating_sub(b.start);
        b_width
            .cmp(&a_width)
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.pattern_index.cmp(&b.pattern_index))
    });
    ordered
}

/// Convert a UTF-8 byte offset into `source` to a UTF-16 code-unit
/// offset, clamping the offset to the source length.
///
/// Counts rather than slices, so no offset can raise: one past the end
/// yields the source's full code-unit length, and one that lands inside
/// a codepoint rounds forward to that codepoint's boundary — a UTF-16
/// range cannot split a surrogate pair.
// [spec:mw:req:uikit.syntax.span-bounds]
#[allow(dead_code)]
pub(crate) fn utf16_offset(source: &str, byte_offset: usize) -> usize {
    let limit = byte_offset.min(source.len());
    let mut units = 0;
    for (index, ch) in source.char_indices() {
        if index >= limit {
            break;
        }
        units += ch.len_utf16();
    }
    units
}

/// Resolve `span` to a UTF-16 `(start, len)` range over `source`, which
/// is the string actually being styled — already trimmed.
///
/// `None` when the span cannot be applied: empty or inverted, ending
/// past the source, or degenerate once converted. A highlighter runs
/// against a copy of the source, so its offsets may no longer describe
/// the string being styled; such a span is dropped, not clamped into a
/// shorter range that would paint the wrong run.
// [spec:mw:req:uikit.syntax.span-bounds]
#[allow(dead_code)]
pub(crate) fn utf16_range(source: &str, span: &SyntaxSpan) -> Option<(usize, usize)> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    let utf16_start = utf16_offset(source, start);
    let utf16_end = utf16_offset(source, end);
    if utf16_end <= utf16_start {
        return None;
    }
    Some((utf16_start, utf16_end - utf16_start))
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

/// The span algebra above is platform-neutral arithmetic, so it is
/// exercised on the host even though its only caller today is the
/// `target_os = "ios"` adapter.
#[cfg(test)]
mod span_tests {
    use super::*;

    fn span(start: u32, end: u32, capture: &str, pattern_index: u32) -> SyntaxSpan {
        SyntaxSpan {
            start,
            end,
            capture: capture.into(),
            pattern_index,
        }
    }

    /// `"aé𝄞b"`: `é` is 2 UTF-8 bytes / 1 UTF-16 unit, and `𝄞`
    /// (U+1D11E) is 4 bytes / 2 units — an astral codepoint that needs a
    /// surrogate pair.
    const MIXED: &str = "aé𝄞b";

    // [spec:mw:req:uikit.syntax.capture-fallback/test]
    #[test]
    fn capture_accent_walks_dotted_prefixes() {
        assert_eq!(capture_accent("keyword"), Some(SyntaxAccent::Keyword));
        assert_eq!(
            capture_accent("keyword.function"),
            Some(SyntaxAccent::Keyword),
            "one level of fallback"
        );
        assert_eq!(
            capture_accent("keyword.control.conditional"),
            Some(SyntaxAccent::Keyword),
            "two levels of fallback, resolved by the shortest prefix"
        );
        assert_eq!(
            capture_accent("function.method.static.builtin"),
            Some(SyntaxAccent::Function),
            "three levels of fallback"
        );
        assert_eq!(
            capture_accent("string.escape"),
            Some(SyntaxAccent::Escape),
            "a whole-name match is preferred over its own prefix, which \
             would have resolved to StringLiteral"
        );
    }

    // [spec:mw:req:uikit.syntax.capture-fallback/test]
    #[test]
    fn unmapped_capture_names_resolve_to_no_accent() {
        assert_eq!(
            capture_accent("nonsense"),
            None,
            "an unrecognised capture is a missing accent, not a placeholder colour"
        );
        assert_eq!(
            capture_accent("nonsense.deeply.nested"),
            None,
            "unmapped at every dotted level"
        );
        assert_eq!(capture_accent(""), None);
        assert_eq!(
            capture_accent("."),
            None,
            "the walk terminates on a bare dot"
        );
        assert_eq!(
            capture_accent("operator"),
            None,
            "deliberately unaccented captures resolve by the same mechanism"
        );
        assert_eq!(capture_accent("punctuation.bracket"), None);
        assert_eq!(capture_accent("variable.parameter"), None);
    }

    // [spec:mw:req:uikit.syntax.span-bounds/test]
    #[test]
    fn utf16_offset_counts_code_units_across_multibyte_codepoints() {
        assert_eq!(MIXED.len(), 8, "byte length");
        assert_eq!(MIXED.encode_utf16().count(), 5, "UTF-16 code-unit length");

        assert_eq!(utf16_offset(MIXED, 0), 0);
        assert_eq!(utf16_offset(MIXED, 1), 1, "after 'a'");
        assert_eq!(utf16_offset(MIXED, 3), 2, "after 2-byte 'é'");
        assert_eq!(
            utf16_offset(MIXED, 7),
            4,
            "after 4-byte '𝄞', which is two UTF-16 units"
        );
        assert_eq!(utf16_offset(MIXED, 8), 5, "whole string");
    }

    // [spec:mw:req:uikit.syntax.span-bounds/test]
    #[test]
    fn utf16_offset_clamps_past_the_end_instead_of_slicing() {
        assert_eq!(utf16_offset(MIXED, 9), 5);
        assert_eq!(utf16_offset(MIXED, usize::MAX), 5);
        assert_eq!(utf16_offset("", 12), 0);
        assert_eq!(
            utf16_offset(MIXED, 5),
            4,
            "an offset inside '𝄞' rounds forward to the codepoint boundary rather \
             than raising, because a UTF-16 range cannot split a surrogate pair"
        );
        assert_eq!(utf16_offset(MIXED, 2), 2, "likewise inside 'é'");
    }

    // [spec:mw:req:uikit.syntax.span-bounds/test]
    #[test]
    fn utf16_range_converts_in_range_spans() {
        assert_eq!(
            utf16_range(MIXED, &span(0, 8, "keyword", 0)),
            Some((0, 5)),
            "whole string, as a UTF-16 (start, len) pair"
        );
        assert_eq!(
            utf16_range(MIXED, &span(3, 7, "keyword", 0)),
            Some((2, 2)),
            "the astral codepoint occupies two UTF-16 units"
        );
        assert_eq!(utf16_range(MIXED, &span(1, 3, "keyword", 0)), Some((1, 1)));
    }

    // [spec:mw:req:uikit.syntax.span-bounds/test]
    #[test]
    fn utf16_range_discards_out_of_range_and_degenerate_spans() {
        assert_eq!(
            utf16_range(MIXED, &span(3, 9, "keyword", 0)),
            None,
            "an end past the source is discarded, not clamped into a shorter range"
        );
        assert_eq!(
            utf16_range(MIXED, &span(0, u32::MAX, "keyword", 0)),
            None,
            "and does not raise"
        );
        assert_eq!(
            utf16_range(MIXED, &span(4, 4, "keyword", 0)),
            None,
            "empty span"
        );
        assert_eq!(
            utf16_range(MIXED, &span(7, 3, "keyword", 0)),
            None,
            "inverted span"
        );
        assert_eq!(
            utf16_range("", &span(0, 1, "keyword", 0)),
            None,
            "no span applies to an empty string"
        );
    }

    // A highlighter runs against a copy of the source: after an edit
    // shortens it, the spans it returns still describe the longer text.
    // [spec:mw:req:uikit.syntax.span-bounds/test]
    #[test]
    fn stale_spans_from_a_longer_source_are_dropped() {
        let edited = "let v";
        let stale = vec![
            span(0, 3, "keyword", 0),
            span(4, 9, "variable", 1),
            span(12, 13, "number", 2),
        ];

        let applied: Vec<(&str, (usize, usize))> = order_spans(&stale)
            .into_iter()
            .filter_map(|s| utf16_range(edited, s).map(|range| (s.capture.as_str(), range)))
            .collect();

        assert_eq!(
            applied,
            vec![("keyword", (0, 3))],
            "only the span still inside the edited source survives"
        );
    }

    // [spec:mw:sem:uikit.syntax.span-priority/test]
    #[test]
    fn order_spans_paints_widest_first() {
        let spans = vec![
            span(2, 6, "variable", 1),
            span(0, 12, "function", 0),
            span(20, 24, "comment", 0),
            span(2, 6, "string", 7),
            span(8, 10, "number", 3),
            span(2, 6, "keyword", 4),
        ];

        let ordered: Vec<&str> = order_spans(&spans)
            .iter()
            .map(|s| s.capture.as_str())
            .collect();

        assert_eq!(
            ordered,
            [
                // widest first, so the enclosing capture is painted
                // before everything nested inside it
                "function", // 0..12
                // equal ranges: ascending pattern_index, so the
                // higher-priority query pattern is painted last and wins
                "variable", // 2..6, pattern 1
                "keyword",  // 2..6, pattern 4
                "string",   // 2..6, pattern 7
                // same width, later start
                "comment", // 20..24
                "number",  // 8..10
            ]
        );
    }
}
