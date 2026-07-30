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

## Rendering spans to HTML

> [spec:mw:req:highlighting.html-delegation]
> Where the provider supplies a span-to-HTML formatter, Markdawn MUST use it
> rather than reimplementing capture-to-slot mapping, coalescing or nesting,
> so that one span vocabulary and one theme vocabulary govern every host.
> Markdawn MUST NOT re-resolve query precedence the engine has already
> applied; spans arrive resolved, and deduplicating them again would discard
> captures the engine deliberately kept.
