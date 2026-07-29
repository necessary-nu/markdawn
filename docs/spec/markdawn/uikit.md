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
