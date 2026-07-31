//! Renderer-neutral syntax-highlighting boundary.
//!
//! Markdawn owns the public span and error types consumed by its renderers.
//! The provider below is bound behind them: no backend type appears in any
//! renderer-facing signature, and the backend crate is not re-exported, so
//! swapping providers is a change to this file rather than to every consumer.

use std::fmt;

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

    /// Take the non-fatal diagnostics produced by the most recent
    /// [`highlight`](Self::highlight) call, leaving none behind.
    ///
    /// A provider can succeed overall while something inside the highlight
    /// did not: an injected language that would not resolve, an exhausted
    /// injection budget. Returning `Ok` while dropping that on the floor is
    /// the "failure becomes absence" this boundary exists to prevent, so it
    /// stays reachable here. Defaulted to empty, because a highlighter with
    /// nothing partial to report is not obliged to invent the concept.
    // [spec:mw:req:highlighting.diagnostics]
    fn take_diagnostics(&mut self) -> Vec<HighlightError> {
        Vec::new()
    }
}

/// The provider binding. Every name from the highlighting backend is
/// confined to this module, which is the whole point of the seam: a
/// renderer cannot accidentally depend on the backend because it cannot see
/// it, and markdawn does not re-export the crate.
// [spec:mw:req:highlighting.provider-binding]
#[cfg(feature = "highlight")]
mod provider {
    use std::sync::Arc;

    use ntreesitter::{
        DynamicRegistry, HighlightEngine, HighlightOutput, Registry, RegistryBuilder,
    };

    use super::{HighlightError, SyntaxHighlighter, SyntaxSpan};

    /// `priority` is the zero-based index of the winning pattern within the
    /// highlights query — the same quantity markdawn calls `pattern_index`
    /// and uses to break ties among equal-range captures.
    // [spec:mw:req:highlighting.span-semantics]
    fn span(span: ntreesitter::Span) -> SyntaxSpan {
        SyntaxSpan {
            start: span.start,
            end: span.end,
            capture: span.capture,
            pattern_index: span.priority,
        }
    }

    fn split(output: HighlightOutput) -> (Vec<SyntaxSpan>, Vec<HighlightError>) {
        (
            output.spans.into_iter().map(span).collect(),
            output
                .diagnostics
                .into_iter()
                .map(|error| HighlightError::new(error.to_string()))
                .collect(),
        )
    }

    /// Highlighter over the language set composed at build time.
    ///
    /// Which languages that is depends on the enabled `ntreesitter-catalog`
    /// features: everything under `highlight-all-languages`, nothing under
    /// bare `highlight`.
    // [spec:mw:req:highlighting.registry-composition]
    pub struct Highlighter {
        engine: HighlightEngine<Registry>,
        diagnostics: Vec<HighlightError>,
    }

    /// Composing the catalog compiles every linked grammar's queries, which
    /// is far too expensive to repeat per highlighter — a caller that builds
    /// an adapter per document would pay it every time. Built once and
    /// cloned; `Registry` is `Arc`-backed, so clones share the compiled
    /// grammars rather than recompiling them.
    ///
    /// The error is stored as a `String` because the registry error type is
    /// not `Clone` and this cell outlives any one caller.
    static SHARED: std::sync::OnceLock<Result<Registry, String>> = std::sync::OnceLock::new();

    fn shared_registry() -> Result<Registry, HighlightError> {
        SHARED
            .get_or_init(|| {
                ntreesitter_catalog::register_all(RegistryBuilder::new())
                    .and_then(RegistryBuilder::build)
                    .map_err(|error| error.to_string())
            })
            .clone()
            .map_err(HighlightError::new)
    }

    impl Highlighter {
        /// Compose every catalogued language into one registry.
        ///
        /// Fallible because registry construction rejects ambiguous aliases
        /// rather than letting one language silently shadow another.
        pub fn new() -> Result<Self, HighlightError> {
            Ok(Self {
                engine: HighlightEngine::new(shared_registry()?),
                diagnostics: Vec::new(),
            })
        }
    }

    impl SyntaxHighlighter for Highlighter {
        fn highlight(
            &mut self,
            language_hint: &str,
            source: &str,
        ) -> Result<Vec<SyntaxSpan>, HighlightError> {
            let output = self
                .engine
                .highlight(language_hint, source)
                .map_err(|error| HighlightError::new(error.to_string()))?;
            let (spans, diagnostics) = split(output);
            self.diagnostics = diagnostics;
            Ok(spans)
        }

        // [spec:mw:req:highlighting.diagnostics]
        fn take_diagnostics(&mut self) -> Vec<HighlightError> {
            std::mem::take(&mut self.diagnostics)
        }
    }

    /// Highlighter over a registry populated at runtime by plugin libraries.
    ///
    /// Takes a fresh snapshot per highlight rather than holding one. A
    /// snapshot is immutable and generation-tracked, so pinning one at
    /// construction would compile, run, and silently never see a language
    /// registered afterwards — which presents as highlighting that works for
    /// some blocks and not others, with nothing in the logs.
    // [spec:mw:req:highlighting.late-registration]
    pub struct DynamicHighlighter {
        registry: Arc<DynamicRegistry>,
        diagnostics: Vec<HighlightError>,
    }

    impl DynamicHighlighter {
        pub fn new(registry: Arc<DynamicRegistry>) -> Self {
            Self {
                registry,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SyntaxHighlighter for DynamicHighlighter {
        fn highlight(
            &mut self,
            language_hint: &str,
            source: &str,
        ) -> Result<Vec<SyntaxSpan>, HighlightError> {
            let engine = HighlightEngine::new(self.registry.snapshot());
            let output = engine
                .highlight(language_hint, source)
                .map_err(|error| HighlightError::new(error.to_string()))?;
            let (spans, diagnostics) = split(output);
            self.diagnostics = diagnostics;
            Ok(spans)
        }

        // [spec:mw:req:highlighting.diagnostics]
        fn take_diagnostics(&mut self) -> Vec<HighlightError> {
            std::mem::take(&mut self.diagnostics)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // An empty registry is a supported configuration, not a broken one:
        // it is what the mobile hosts ship before any plugin loads.
        // [spec:mw:req:highlighting.registry-composition/test]
        // [spec:mw:req:highlighting.error-fallback/test]
        #[test]
        fn an_unregistered_language_is_an_error_not_empty_spans() {
            let registry = Arc::new(DynamicRegistry::new(
                RegistryBuilder::new().build().expect("empty registry"),
            ));

            let error = DynamicHighlighter::new(registry)
                .highlight("rust", "fn main() {}")
                .expect_err("no grammar is registered");

            assert!(
                error.message().contains("rust"),
                "the diagnostic should name the language it could not resolve, got {:?}",
                error.message()
            );
        }

        // The hazard highlighting.late-registration exists to forbid: a
        // registry snapshot is immutable, so a highlighter that took one at
        // construction would answer this second call from the stale view and
        // report the language as unsupported forever.
        // [spec:mw:req:highlighting.late-registration/test]
        #[cfg(feature = "highlight-all-languages")]
        #[test]
        fn a_highlighter_observes_a_language_registered_after_it_was_built() {
            let registry = Arc::new(DynamicRegistry::new(
                RegistryBuilder::new().build().expect("empty registry"),
            ));
            let mut highlighter = DynamicHighlighter::new(Arc::clone(&registry));
            assert!(highlighter.highlight("json", "{}").is_err());

            let json = ntreesitter_catalog::DESCRIPTORS
                .iter()
                .find(|descriptor| descriptor.canonical_id == "json")
                .copied()
                .expect("the catalog ships json under highlight-all-languages");
            registry.register(json).expect("register json");

            let spans = highlighter
                .highlight("json", "{\"a\": 1}")
                .expect("the same highlighter now resolves json");
            assert!(!spans.is_empty());
        }

        // [spec:mw:req:highlighting.span-semantics/test]
        // [spec:mw:req:highlighting.registry-composition/test]
        #[cfg(feature = "highlight-all-languages")]
        #[test]
        fn spans_are_utf8_byte_offsets_into_the_original_source() {
            let source = "// é\nfn main() {}\n";
            let spans = Highlighter::new()
                .expect("compose the catalog")
                .highlight("rust", source)
                .expect("rust is catalogued");

            assert!(!spans.is_empty());
            for span in &spans {
                assert!(span.start < span.end, "half-open and non-inverted");
                assert!(span.end as usize <= source.len(), "within the source");
                assert!(
                    source.is_char_boundary(span.start as usize)
                        && source.is_char_boundary(span.end as usize),
                    "{span:?} splits a codepoint, so the offsets are not UTF-8 byte offsets"
                );
            }
        }
    }
}

#[cfg(feature = "highlight")]
pub use provider::{DynamicHighlighter, Highlighter};

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
        // `definition.function` and `constructor` are the provider's
        // vocabulary for what the older one called `function.definition` and
        // (in places) `function`. Without these arms the prefix walk falls
        // through to `definition` / no match and the run renders as plain
        // text — a silent loss measured at several hundred bytes across the
        // parity corpus, all of it code the previous backend coloured.
        "function"
        | "function.builtin"
        | "function.macro"
        | "method"
        | "definition.function"
        | "constructor" => SyntaxAccent::Function,
        "type" | "type.builtin" | "namespace" | "module" | "class" | "definition.type" => {
            SyntaxAccent::Type
        }
        "string" | "string.special" | "character" => SyntaxAccent::StringLiteral,
        "comment" => SyntaxAccent::Comment,
        "constant" | "constant.builtin" | "boolean" | "number" | "float" => SyntaxAccent::Constant,
        // Deliberately unaccented: syntax that reads better in the base
        // text colour than in an accent of its own. Named rather than
        // left to the catch-all so the intent survives editing.
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => {
            return None;
        }
        // `parameter` is the provider's spelling of `variable.parameter`;
        // both are deliberately unaccented, and naming it keeps that a
        // decision rather than an unmapped-name accident.
        "variable" | "variable.parameter" | "variable.builtin" | "parameter" => return None,
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

/// Render neutral spans as HTML through the provider's formatter.
///
/// Delegated rather than reimplemented so one capture vocabulary and one
/// theme vocabulary govern every host. `HtmlFormat::CustomElements` is the
/// `<a-k>`/`<a-s>` output existing theme CSS is written against.
///
/// The spans arrive already resolved — the engine picked a winner per range
/// before emitting — so this deliberately does not re-run precedence.
// [spec:mw:req:highlighting.html-delegation]
#[cfg(feature = "highlight")]
pub(crate) fn spans_to_html(source: &str, spans: Vec<SyntaxSpan>) -> String {
    let spans: Vec<ntreesitter_highlight::Span> = spans
        .into_iter()
        .map(|span| ntreesitter_highlight::Span {
            start: span.start,
            end: span.end,
            capture: span.capture,
            priority: span.pattern_index,
        })
        .collect();
    ntreesitter_highlight::spans_to_html(
        source,
        &spans,
        &ntreesitter_highlight::HtmlFormat::CustomElements,
    )
}

#[cfg(all(test, feature = "highlight"))]
mod tests {
    use super::*;

    struct Stub;

    impl SyntaxHighlighter for Stub {
        fn highlight(
            &mut self,
            _language_hint: &str,
            _source: &str,
        ) -> Result<Vec<SyntaxSpan>, HighlightError> {
            Ok(Vec::new())
        }
    }

    // An implementation with nothing partial to report is not obliged to
    // invent the concept, which is why the method is defaulted rather than
    // required.
    // [spec:mw:req:highlighting.diagnostics/test]
    #[test]
    fn diagnostics_default_to_none_for_a_highlighter_that_has_none() {
        assert!(Stub.take_diagnostics().is_empty());
    }

    // Byte offsets are UTF-8, so the multibyte prefix must not shift the
    // span, and the tag vocabulary is the legacy custom-element one existing
    // theme CSS targets.
    // [spec:mw:req:highlighting.html-delegation/test]
    #[test]
    fn html_delegation_emits_legacy_custom_elements_over_byte_ranges() {
        let spans = vec![SyntaxSpan {
            start: 2,
            end: 6,
            capture: "string".into(),
            pattern_index: 7,
        }];

        assert_eq!(spans_to_html("éname", spans), "é<a-s>name</a-s>");
    }

    // The previous revision of this test fed two spans over one range and
    // asserted the higher pattern index won. That input is no longer
    // reachable: the engine resolves precedence before emitting, and
    // highlighting.html-delegation forbids the renderer re-running it, so a
    // formatter handed competing equal-range spans is being asked a question
    // the pipeline cannot pose.
    // [spec:mw:req:highlighting.html-delegation/test]
    #[test]
    fn html_delegation_escapes_unstyled_source() {
        assert_eq!(spans_to_html("a<b & c", Vec::new()), "a&lt;b &amp; c");
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
