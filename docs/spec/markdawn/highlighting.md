# highlighting — renderer-neutral syntax-highlighting boundary

markdawn consumes semantic highlighting spans for fenced code blocks but does
not own the parser runtime, the language catalog, or the application's provider
policy. `src/highlight.rs` is the seam: it declares markdawn's own span, error
and highlighter types so that every renderer (`html`, `iced`, `uikit`,
`dioxus`, `ansi`) codes against markdawn types and never against whichever
highlighting engine the host happened to link.

These rules govern that seam only. What a renderer *does* with the spans it
receives is the renderer's own concern and lives in that adapter's file.

## Boundary types

> [spec:mw:req:highlighting.api]
> Markdawn MUST expose public, renderer-neutral `SyntaxHighlighter`,
> `SyntaxSpan`, and `HighlightError` types. The boundary MUST use only
> Markdawn-owned types and MUST return
> `Result<Vec<SyntaxSpan>, HighlightError>`.

> [spec:mw:req:highlighting.span-semantics]
> `SyntaxSpan` MUST preserve UTF-8 byte offsets, half-open `[start, end)`
> ranges, raw capture names, overlapping and nested spans, and the
> query-pattern priority used to resolve otherwise equivalent captures.

## Failure handling

> [spec:mw:req:highlighting.error-fallback]
> A syntax-highlighting failure MUST remain a `HighlightError` at the
> highlighter boundary. Each renderer MUST catch that error and render the
> original source as plain code.

> [spec:mw:req:highlighting.diagnostics]
> A provider that distinguishes total failure from partial degradation MUST
> NOT have the degraded case flattened into silence. Where a highlight
> succeeds but something inside it did not — an injected language that would
> not resolve, an exhausted injection budget — those diagnostics MUST remain
> reachable from the boundary alongside the spans they accompany, rather than
> being discarded because the call returned `Ok`. Renderers are not obliged to
> display them; they are obliged not to be the reason the information is
> unobtainable.

## Provider binding

> [spec:mw:req:highlighting.provider-binding]
> A highlighting backend MUST reach renderers only through `SyntaxHighlighter`.
> Backend span and error types MUST NOT appear in any renderer-facing
> signature, and Markdawn MUST NOT re-export the backend crate from its own
> public API. The adapter MUST preserve every span field and the provider
> diagnostic.

> [spec:mw:req:highlighting.registry-composition]
> Markdawn MUST support both a statically composed language set, resolved at
> build time from the provider's catalog, and an initially empty one populated
> at runtime. Which applies is a build-time feature choice, not a runtime mode:
> a host that bakes in no grammars MUST NOT pay their binary cost.

> [spec:mw:req:highlighting.late-registration]
> A highlighter built over a runtime-populated language set MUST observe
> languages registered after it was constructed. Holding an immutable view
> taken at construction time is a conforming implementation only if that view
> is refreshed per highlight; a code block rendered after its language
> registers MUST be highlighted.

## Resolving spans for painting

These three govern every renderer that paints spans itself rather than
delegating to the provider's HTML formatter — today `uikit` and `iced`.
They lived under `uikit` while it was the only such renderer, which let the
two drift: iced resolved capture names by first dotted component rather
than by prefix walk, and clamped out-of-range spans where UIKit discarded
them. Two renderers of one document disagreeing about which bytes are
styled is a conformance difference, not a detail.

> [spec:mw:req:highlighting.capture-accent]
> A capture name that has no accent of its own MUST be retried against
> successively shorter dotted prefixes — `keyword.function` falls back to
> `keyword` — and a name still unmapped after the walk MUST leave the run
> unaccented so it keeps the base text colour. Taking only the first dotted
> component is not a conforming implementation: it makes `string.escape`
> indistinguishable from `string`. Capture vocabularies differ per grammar
> and grow over time, so an unrecognised capture is a missing accent, not an
> error, and MUST NOT be rendered in a default or placeholder colour.
> Captures that are deliberately unaccented — operators, punctuation, plain
> variables — resolve to no accent by the same mechanism. Renderers MUST
> resolve to the shared accent vocabulary rather than to their own palette
> keys, so that two renderers cannot disagree about what a capture means.

> [spec:mw:sem:highlighting.span-priority]
> Spans may overlap and nest, so a byte covered by several MUST resolve to
> exactly one: the narrowest span covering it, ties broken by the later
> start and then the higher `pattern_index`. An enclosing capture is
> therefore overpainted by the captures nested inside it, and for equal
> ranges the higher-priority query pattern wins. A renderer MAY paint in
> sequence and let the last write stand, or select a winner per run, but
> both MUST derive that choice from one ordering — the two formulations
> disagree on ties whenever they are written separately.

> [spec:mw:req:highlighting.span-bounds]
> A span MUST be discarded — never clamped into a shorter range — when it
> is empty or inverted (`start >= end`) or when its end exceeds the source
> length. A highlighter runs against a copy of the source, so after an edit
> shortens it the returned offsets still describe the longer text; clamping
> such a span paints a real capture over the wrong bytes, which reads as a
> rendering bug, whereas discarding it merely leaves the run unstyled.
> Out-of-range spans MUST NOT raise.

## Rendering spans to HTML

> [spec:mw:req:highlighting.html-delegation]
> Where the provider supplies a span-to-HTML formatter, Markdawn MUST use it
> rather than reimplementing capture-to-slot mapping, coalescing or nesting,
> so that one span vocabulary and one theme vocabulary govern every host.
> Markdawn MUST NOT re-resolve query precedence the engine has already
> applied; spans arrive resolved, and deduplicating them again would discard
> captures the engine deliberately kept.
