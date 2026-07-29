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

## Provider transition

> [spec:mw:req:highlighting.arborium-transition]
> While Markdawn retains its legacy Arborium feature, Arborium highlighters
> MUST adapt to `SyntaxHighlighter` without exposing Arborium span or error
> types to renderers, and the adapter MUST preserve every span field and the
> provider diagnostic.
