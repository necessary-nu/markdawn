# uikit — UIKit/iOS rendering adapter

This document specifies `src/adapter/uikit/`: the adapter that turns
markdawn's `Node` tree into UIKit view hierarchies and
`NSAttributedString` runs. It is a mapping contract, not a layout
engine — TextKit does the line breaking; this adapter decides which
markdown construct becomes an attributed-string run inside a shared
`UITextView` and which becomes a standalone `UIView`, and which
attributes each run carries.

Rules reference `UIKitStyle` as the single record from which every
font, colour, and spacing decision is resolved.

## Rendering surface

> [spec:mw:def:uikit.surface.text-renderer]
> A `TextRenderer<R>` converts *text-like* markdown blocks into a
> `NSMutableAttributedString`. `R` is a `RenderUIKitCustom<Ext>`
> implementation invoked for `Node::Custom`; the unit type `()` is
> the default and reports `UIKitError::UnsupportedCustomNode` for
> every custom payload. The renderer holds an `Arc<UIKitStyle>`, so
> a style is shared by every renderer and view factory built from
> the same sink rather than cloned per block.

> [spec:mw:def:uikit.surface.render-ctx]
> A `RenderCtx` is the inherited context threaded down one render
> pass. It carries `blockquote_depth` (nesting level used for
> paragraph indentation), `list_stack` (a `ListFrame` per enclosing
> list, each recording `ordered`, `start`, `item_index`, `tight`),
> `provisional` (the emission is mid-stream and not yet final),
> `is_last_list_item` and `is_last_block` (used to suppress trailing
> paragraph spacing), and `pending_images` (a shared queue of
> `PendingImage` records the sink drains after insertion). A
> `RenderCtx` is rebuilt per child rather than mutated in place, so
> a sibling never observes a descendant's depth or list frame.

> [spec:mw:def:uikit.surface.block-sink]
> A `BlockSink<R>` is the `Sink<Node<Ext>>` implementation for
> UIKit. It owns a vertical `UIStackView` whose arranged subviews
> are *compartments*, a `TextRenderer<R>`, and an `ElementMap`
> keyed by `CorrelationId` that records, for every block the sink
> has seen, which compartment renders it. `StreamItem::Upsert`
> whose id is already mapped updates that block in place; an unseen
> id appends; `StreamItem::Delete` removes it. The stack view is
> the sole embedding point exposed to the host application.

> [spec:mw:def:uikit.surface.element-map]
> An `ElementMap` is an insertion-ordered map from `CorrelationId`
> to an `ElementLocation`, which is either `TextRun {
> compartment_index }` or `BlockView { compartment_index }`. It
> records only *which* compartment renders a block, never where
> inside it: a text-like block's `NSRange` lives in that
> compartment's own `blocks` vector, so an update resolves the
> compartment through the map and the range through the
> compartment. The indirection exists because a `CorrelationId` is
> stable for the life of a block while its compartment index is not
> — emptying a `TextRun` deletes the whole compartment and
> renumbers every compartment after it — so one pass of
> `shift_compartment_indices` over the map repairs every affected
> location, rather than indices being chased through the stack
> view. Deletion is a swap-remove, so map order after a delete no
> longer matches emission order.

## Block-to-compartment mapping

> [spec:mw:req:uikit.compartments.text-run-coalescing]
> Blocks MUST be partitioned into text-like and non-text kinds.
> Text-like blocks (paragraphs, headings, lists and their items,
> inline-level nodes promoted to block position, HTML, math blocks,
> and custom nodes) MUST be appended to the trailing compartment
> when that compartment is already a `TextRun`, so consecutive
> text-like blocks share one `UITextView` and one
> `NSTextContentStorage`; otherwise a new `TextRun` compartment is
> created. Non-text blocks (`Table`, `CodeBlock`, `BlockQuote`,
> `ThematicBreak`) MUST each become their own `BlockView`
> compartment built by a `views::*` factory, and MUST NOT be merged
> into a neighbouring text run — passing one to the text renderer
> yields `UIKitError::UnsupportedBlockNode`. A `BlockView`
> compartment MUST be given `UIStackView` custom spacing equal to
> `UIKitStyle::paragraph_spacing`, because a text run's trailing
> gap comes from its `NSParagraphStyle` and a view has no such
> attribute. When the last block in a `TextRun` is deleted the
> compartment itself MUST be removed from the stack, and every
> compartment index recorded in the `ElementMap` at or after the
> removed position MUST be shifted to stay consistent with the
> stack.

> [spec:mw:req:uikit.compartments.range-bookkeeping]
> Each block inside a `TextRun` MUST record the `NSRange` it
> occupies in the shared text storage. Replacing or deleting a
> block MUST rewrite that block's own range and MUST shift the
> `location` of every later entry in the same run by the length
> delta; otherwise a subsequent edit addresses the wrong
> characters. Replacement MUST take an append-only fast path when
> the newly rendered text is a prefix-extension of the text
> currently in storage: only the tail delta is inserted. Streaming
> re-emits a growing block on every token, so a full replace per
> emission is quadratic in document length. Both paths drain
> `RenderCtx::pending_images` afterwards, so an image that first
> appears in the appended tail is still registered with the async
> loader.

## Style resolution and attribute mapping

> [spec:mw:req:uikit.style.system-colors]
> Every colour in a default `UIKitStyle` MUST be an iOS *semantic*
> system colour — `labelColor` for body and code text,
> `secondarySystemFillColor` for code backgrounds, `separatorColor`
> for blockquote accents and rules, `linkColor` for links — and
> view factories MUST likewise use semantic colours
> (`systemBackgroundColor`, `secondarySystemBackgroundColor`,
> `tertiaryLabelColor`) rather than literal RGB. Semantic colours
> resolve against the trait collection at draw time, so light/dark
> mode and increased-contrast changes take effect without the host
> re-rendering the document or rebuilding any attributed string.

> [spec:mw:req:uikit.attributes.font-resolution]
> Body and code fonts MUST resolve from `UIKitStyle`: when
> `body_font_name` / `code_font_name` names a font that
> `UIFont::fontWithName_size` can instantiate that font is used,
> and otherwise the corresponding system font
> (`systemFontOfSize` / `monospacedSystemFontOfSize_weight`) is
> substituted — an unavailable font name MUST degrade to the system
> face rather than fail the render. When `use_dynamic_type` is set
> the resolved font MUST then be scaled through
> `UIFontMetrics::defaultMetrics`. Dynamic Type scaling MUST NOT be
> applied twice: a code span nested in already-scaled text resolves
> its monospaced face against the base font's `pointSize` and skips
> the metrics step, because re-scaling an already-scaled size
> compounds the user's text-size preference.

> [spec:mw:sem:uikit.attributes.inherited-traits]
> Emphasis and strength are expressed as symbolic-trait additions
> to the *inherited* font rather than as fixed faces, so nesting
> composes: strong inside emphasis yields a bold-italic descriptor
> because the italic trait is already present on the base font when
> the bold trait is added. When the descriptor cannot produce a
> font carrying the combined traits, the unmodified base font is
> returned unchanged for trait addition, and the system italic face
> at the same point size is substituted for italics. Headings scale
> the body size by `heading_scale[level - 1]`, clamping the index
> at 5 so a level beyond the array is styled as the smallest
> heading, and then add the bold trait.

## Syntax highlighting

> [spec:mw:req:uikit.syntax.capture-fallback]
> A capture name that has no colour of its own MUST be retried
> against successively shorter dotted prefixes — `keyword.function`
> falls back to `keyword` — and a name that is still unmapped after
> the walk MUST leave the run's colour untouched so it keeps the
> base text colour. Capture vocabularies differ per grammar and
> grow over time; an unrecognised capture is a missing accent, not
> an error, and MUST NOT be rendered in a default or placeholder
> colour. Captures that are deliberately unaccented (operators,
> punctuation, plain variables) resolve to no colour by the same
> mechanism.

> [spec:mw:sem:uikit.syntax.span-priority]
> Spans may overlap and nest, and the attributed string keeps
> whichever colour was applied last for a given character. Spans
> are therefore applied widest-first, ties broken by ascending
> start offset and then by ascending `pattern_index`, so an
> enclosing capture is painted before the captures nested inside it
> and, for equal ranges, the higher-priority query pattern is
> applied last and wins.

> [spec:mw:req:uikit.syntax.span-bounds]
> Span offsets are UTF-8 byte offsets into the source; attributed
> string ranges are UTF-16 code-unit offsets, so every offset MUST
> be converted before use and the conversion MUST clamp its input
> to the source length rather than slicing out of bounds. The
> source is trimmed of trailing newlines before highlighting, and a
> span MUST be discarded — not clamped into a shorter range — when
> it is empty or inverted (`start >= end`), when its end exceeds
> the trimmed byte length, or when the converted UTF-16 range is
> degenerate. A highlighter runs against a copy of the source and
> can return offsets that no longer describe the string being
> attributed; out-of-range spans MUST leave the run unstyled rather
> than raise.

## Block layout

> [spec:mw:req:uikit.layout.list-item]
> A list item MUST carry a paragraph style whose
> `firstLineHeadIndent` is `list_indent × (depth - 1)` and whose
> `headIndent` is `list_indent × depth`, so the marker hangs in the
> gutter and wrapped content aligns under the item's text rather
> than under the marker. The marker text is rendered inline ahead
> of the item content: `start + item_index` followed by `". "` for
> an ordered list, a bullet for an unordered one, and an additional
> ballot-box glyph reflecting `checked` for a task item. Trailing
> spacing MUST be `tight_list_spacing` for an item in a tight list,
> `paragraph_spacing` otherwise, and zero when the item is the last
> block — except that the final item of a tight list uses the
> looser spacing so the list separates from what follows it.

> [spec:mw:req:uikit.layout.blockquote]
> Blockquote nesting is expressed two different ways and the two
> MUST NOT both apply to the same content. Text rendered *inside*
> the attributed-string path adds `blockquote_indent ×
> blockquote_depth` to both `firstLineHeadIndent` and `headIndent`
> of every paragraph style, which is how alerts and definition
> descriptions are indented. A `Node::BlockQuote` reaching the sink
> instead becomes its own view — an accent bar coloured
> `blockquote_accent_color` pinned to the container's full height
> with the content text view inset to its right — and its children
> MUST be rendered with `blockquote_depth` reset to zero, because
> the view already supplies the visual offset and adding the
> paragraph indent would double it.

> [spec:mw:def:uikit.layout.code-block-view]
> A code block renders as a container `UIView` with
> `code_background_color` and a rounded corner, holding a
> multi-line `UILabel` inset on all four sides. The label carries
> the highlighted attributed string when the highlighter produced
> one, and otherwise the literal source — trailing newlines trimmed
> — in the resolved monospaced font at `code_text_color`. A
> non-empty info string additionally produces a language badge
> pinned to the top-trailing corner in a smaller monospaced face at
> `tertiaryLabelColor`, and pushes the code label down so the two
> do not overlap. The container's accessibility label names the
> block and its language.

## Images and async loading

> [spec:mw:sem:uikit.media.image-source+1]
> An image destination MUST first satisfy the crate's image URL
> allow-list — the same `is_safe_image_url` that gates the adapters
> emitting markup, which rejects a destination containing any ASCII
> control character, admits only allow-listed schemes, and relaxes
> to `data:` solely for a `data:image/` prefix. A destination that
> fails it yields no image, so a `data:text/html;…` payload and a
> control-character `javascript:` smuggling attempt are refused
> here exactly as they are there. A destination that passes is
> resolved by three ordered attempts: a `data:` URI is decoded and
> rasterised inline; otherwise a destination already in the loader's
> image cache produces its attachment immediately; otherwise a
> destination beginning with the literal `http://` or `https://`
> produces a placeholder attachment and queues a `PendingImage` on
> the `RenderCtx`. A decoded `data:` payload reaches the SVG
> rasteriser only when its media type says SVG: the header up to the
> first `;`, parameters dropped and surrounding whitespace trimmed,
> MUST equal `image/svg+xml` or `image/svg` case-insensitively —
> a whole-value comparison, never a substring search, so a header
> carrying the letters `svg` only in a parameter or a longer subtype
> cannot route its bytes into the SVG parser. Every other payload
> goes to `UIImage`'s decoder. Anything else — and any of the three
> attempts that fails to produce an image — falls back to the alt
> text rendered as `[alt]` in the surrounding font, so a destination
> that cannot be displayed still occupies its place in the run.

> [spec:mw:sem:uikit.image.async-load]
> A queued `PendingImage` is fetched once per URL. The loader holds
> a list of `(attachment, content storage)` pairs keyed by URL and
> starts an `NSURLSession` data task only when that list was empty,
> so several attachments sharing a URL cost one request and are all
> updated from its single completion. Response bytes that sniff as
> SVG are rasterised through CoreSVG; everything else goes to
> `UIImage::imageWithData`. A decoded image enters a process-wide
> cache bounded at 100 entries, evicting an arbitrary existing key
> when full — that cache is what lets a re-render during streaming
> resolve the same URL synchronously. Application always happens on
> the main thread: pending entries are held in `MainThreadBound` so
> they can cross the completion block's thread, and each is applied
> by setting the attachment's image, clamping its bounds to 600
> points wide with the aspect ratio preserved, then invalidating
> layout. Setting an image on an existing `NSTextAttachment` fires
> no text-storage notification, so invalidation additionally
> rewrites one character's attributes to drive the `processEditing`
> chain that makes TextKit 2 redraw the glyph. A transport error, a
> null body, or a payload neither CoreSVG nor `UIImage` can decode
> substitutes an `exclamationmark.triangle` symbol; the placeholder
> is the `photo` symbol; either is simply absent, not replaced by a
> fallback drawing, when SF Symbols yields nothing.

> [spec:mw:req:uikit.image.cancellation]
> Every URL registered from a block MUST be recorded against that
> block, and removing the block — or removing the compartment that
> held it — MUST cancel each URL so recorded. Cancellation MUST
> cancel the in-flight task, drop the loader's pending entries for
> that URL, and be safe to call from any thread. The pending
> entries are the only path from a completion back to an
> attachment, so a response that arrives after cancellation finds
> nothing to apply and returns without touching a view whose text
> storage may already be gone; the guarantee is that no completion
> outlives its attachment, not that the transfer stops in time.
> Cancellation MUST NOT evict the cached image, so a URL that
> loaded, was cancelled and is re-emitted still resolves from
> cache. A `BlockView` compartment records its pending URLs for
> cancellation but starts no load, because it owns no
> `NSTextContentStorage` whose layout could be invalidated.

## Data URIs and SVG rasterisation

> [spec:mw:def:uikit.media.data-uri]
> A `data:` URI parses as `data:[<mediatype>][;base64],<data>`: the
> prefix is required, the first comma terminates the header, and a
> `;base64` suffix on the header selects base64 decoding over
> percent-decoding for the payload. An absent media type yields
> `text/plain;charset=US-ASCII`; any other header is returned
> verbatim, parameters included, and is never validated as a media
> type. ASCII whitespace anywhere inside a base64 payload is
> stripped before decoding, because generators wrap long payloads
> across lines. Parsing yields nothing — never a partial or empty
> result — when the prefix is absent, when there is no comma, or
> when base64 decoding fails; percent-decoding has no failure mode
> and passes an invalid escape through as its literal bytes.

> [spec:mw:req:uikit.media.svg-raster+1]
> SVG reaches UIKit as a `UIImage`, never as a view. Bytes are
> recognised as SVG only when an opening `<svg` appears within the
> first kilobyte and a closing `</svg>` within the last,
> case-insensitively, each tolerating prolog or trailing junk
> around it; requiring both ends keeps unrelated XML that merely
> contains the substring from being handed to the rasteriser.
> CoreSVG is private API, so `CGSVGDocumentCreateFromData`,
> `CGSVGDocumentRelease` and `_imageWithCGSVGDocument:` MUST be
> resolved at run time — by `dlsym` and selector registration over
> base64-obscured names, once, behind a `OnceLock` — and a failed
> resolution MUST degrade to no image rather than abort, because
> these symbols are not contractual. The document MUST be released
> once the image is built, and the autoreleased result retained. A
> rendered image MUST be rejected when its canvas is non-finite,
> non-positive, or exceeds 4096 points on either axis, since the
> attachment bounds taken from it drive TextKit layout.
>
> SVG that originated outside this crate MUST NOT reach CoreSVG
> unchecked: a raw HTML chunk in the document, a decoded `data:`
> payload and the body of a remote fetch are all attacker-shaped
> input to a private XML parser. Such bytes MUST be refused when
> they exceed `MAX_SVG_BYTES` or are not valid UTF-8 — the
> sanitiser reads text, and a byte stream it cannot read is dropped
> rather than forwarded — then passed through `svg_sanitize`, with
> only its output handed to the rasteriser; an input the sanitiser
> empties out yields no image. Both the cap and the sanitiser apply
> to the source, before any parse, so the canvas rejection above is
> a second bound rather than the first. The unsanitised entry point
> remains only for SVG this crate produced itself: math typesetting
> and the diagram-engine views.
>
> Failure at any step yields no image and each caller degrades on
> its own terms — an HTML chunk that sniffs as SVG but does not
> rasterise falls back to showing its source in the monospaced
> face.

## Math and diagram views

> [spec:mw:sem:uikit.math.typeset]
> Math is typeset by rendering the LaTeX source to SVG through
> `mathml-core` and rasterising that SVG by the same CoreSVG path
> as any other image, producing an attachment inside the text run;
> there is no native math view. The renderer, and the math font it
> bundles, is built once and shared across calls. Results are
> cached under the source's hash paired with the font size
> quantised to hundredths of a point, bounded at 200 entries with
> arbitrary eviction, so a streaming re-emission of an unchanged
> formula re-runs neither layout nor rasterisation. Size follows
> display: inline math takes the surrounding font's point size,
> block math the body size scaled by `math_block_scale`. When
> typesetting or rasterisation fails the source is emitted as
> literal text in the monospaced face, so an unparseable formula
> shows its own LaTeX rather than vanishing.

> [spec:mw:req:uikit.diagram.view-factory+1]
> Fenced code blocks tagged `pikchr`, `dot`/`graphviz` or `typst`
> are rendered as diagrams when the matching opt-in is set on the
> sink. Each dispatches to a factory that obtains SVG from the
> crate's shared entry point for that engine, rasterises it, and
> returns a `UIImageView` in aspect-fit mode carrying a
> height-to-width constraint equal to the image's intrinsic ratio —
> the image is vector, so that constraint is what gives the view a
> height at any width — and an accessibility label naming the
> diagram kind.
>
> Going through the shared entry point is a requirement, not a
> convenience. Engine input is document text, so engine output is
> attacker-shaped, and that shared tail is where the output size
> cap, the SVG sanitiser and text-to-path outlining apply. A
> factory MUST NOT call its engine directly; it MUST obtain the
> same bytes any other adapter would for the same source. Because
> those bytes are the sanitiser's own output they are handed to the
> plain rasteriser rather than the sanitising one — re-sanitising
> SVG this crate just sanitised would only buy a second parse.
> Outlining means diagram glyphs are drawn as paths instead of
> being resolved against a font when CoreSVG rasterises, which both
> drops that font dependency and costs nothing: the product is an
> image with no selectable text either way.
>
> A factory MUST yield nothing, rather than an error or an empty
> view, when its engine fails, when the rendered SVG exceeds the
> size cap, or when the SVG does not rasterise, and the sink MUST
> then fall through to the ordinary code-block view, so a malformed
> or oversized diagram displays as its own highlighted source.
> Dispatch MUST be skipped while the code block is provisional: a
> mid-stream fence has not yet received its closing delimiter, and
> feeding partial source to an engine only yields a fresh syntax
> error per token.
