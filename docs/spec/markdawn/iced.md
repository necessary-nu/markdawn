# iced — GUI rendering adapter and text selection

Behaviour of `src/adapter/iced/`: the `Renderer` that turns parsed
markdown into an `iced::Element` tree, and the `SelectableRichText`
widget plus `SelectionGroup` wrapper that give the rendered document
browser-style drag-to-select and copy.

Coordinate conventions used throughout. *Window coordinates* are the
coordinates iced reports for the mouse cursor; *widget-local
coordinates* are relative to a widget's layout bounds origin;
*flat-text-global byte offsets* are byte offsets into one widget's
concatenated span text. cosmic-text reports glyph and cursor byte
offsets that are *line-local* — relative to the start of the buffer
line they belong to.

## Widget model

> [spec:mw:def:iced.widget.selectable-rich-text]
> A `SelectableRichText` is one rich-text block of the rendered
> document: a vector of iced text spans laid out by cosmic-text,
> augmented with drag-to-select highlighting, selected-text fragment
> publishing, optional per-span link click handling, and inline
> replaced atoms. Every rich-text block emitted by a `Renderer` is
> one of these widgets.

> [spec:mw:def:iced.widget.selection-handle]
> A `Selection` is a cheap-to-clone handle
> (`Rc<RefCell<SelectionState>>`) to the cross-block selection
> state: a pending anchor, a promoted anchor, a cursor position —
> all in window coordinates — and a `finalized` flag. One `Renderer`
> owns exactly one `Selection` and clones it into every widget it
> emits, which is what makes a drag that starts in one block and
> ends in another a single contiguous selection rather than two
> independent ones. Nothing about the selection is stored per
> widget: each widget re-derives its own share of those two points
> on every draw and every `operate` traversal, so selection survives
> relayout and scrolling without per-block bookkeeping.

> [spec:mw:def:iced.widget.selection-group]
> A `SelectionGroup` is a transparent wrapper widget that owns the
> whole rendered region. It delegates every `Widget` method to its
> inner element and additionally arbitrates mouse presses, so that
> clicks landing in dead space — between blocks, next to list
> bullets, in table gutters — still drive the shared selection
> instead of falling through unhandled.

> [spec:mw:def:iced.widget.inline-atom]
> An `InlineAtom` is either a `Span` (a normal text span, shaped by
> cosmic-text like any other) or a `Replaced` atom: SVG bytes with
> canonical `copy_text`, intrinsic `em_width` / `em_height` /
> `em_baseline` metrics, an optional colour filter, and an optional
> link href. A `Replaced` atom occupies space in the inline flow the
> way a browser inline-block element does, and is painted over that
> space during draw.

> [spec:mw:def:iced.widget.text-tables]
> Each widget's cached state carries three tables derived from its
> spans: `flat_text` (all span text concatenated), `span_offsets`
> (the start byte of each span within `flat_text`), and
> `line_offsets` (the start byte within `flat_text` of each
> cosmic-text buffer line — 0 followed by the offset after every
> `\n`). Selection, hit-testing and copy all address text through
> `flat_text` byte offsets, and `line_offsets` is the conversion
> table from cosmic-text's line-local offsets into that space.

## Selection state machine

> [spec:mw:sem:iced.selection.press-resets]
> A left-button press records the press point as the *pending*
> anchor and clears the promoted anchor, the cursor and the
> `finalized` flag. Any previously completed selection therefore
> disappears at press time, before the button is released.

> [spec:mw:req:iced.selection.drag-threshold]
> A pending anchor MUST NOT be promoted to a real selection anchor
> until the cursor has moved at least `DRAG_THRESHOLD_PX` (4.0) from
> the press point in Manhattan distance (`|dx| + |dy|`). Below the
> threshold the press is still a click: no anchor, no highlight, no
> copyable text. This defeats the sub-pixel hand drift that would
> otherwise leave a one-character ghost highlight behind every
> click. On promotion the anchor takes the *press* point, not the
> point that crossed the threshold, and the cursor takes the current
> point; from then on cursor movement updates only the cursor and
> the anchor stays pinned for the rest of the drag.

> [spec:mw:sem:iced.selection.release-finalizes]
> Releasing the left button clears the pending anchor and, if an
> anchor had been promoted, sets `finalized`. A finalized selection
> ignores further cursor movement, so the highlight stays frozen
> until the next press.

> [spec:mw:req:iced.selection.click-yields-no-selection]
> A press-and-release that never crossed the drag threshold MUST
> leave no selection: the pending anchor is discarded and the
> selection range is empty. A plain click therefore clears the
> current selection, and repeated clicks at the same point —
> double-click, triple-click — are not special-cased into word or
> paragraph selection: each press simply resets the state machine.

> [spec:mw:sem:iced.selection.drag-routing]
> The selection reports itself as *dragging* while either a pending
> or a promoted anchor exists and it is not finalized. Both the
> widget and the group wrapper gate their cursor-moved handling on
> that predicate, so a press that has not yet crossed the threshold
> still receives cursor updates and can still be promoted.

> [spec:mw:req:iced.selection.range-ordering]
> The selection range is reported as an ordered pair of window
> points. The anchor and cursor MUST be swapped when needed so the
> first point is the lesser under lexicographic `(y, x)` comparison
> — reading order, row before column — and the range MUST be absent
> unless both an anchor and a cursor are set. A backwards drag
> therefore yields exactly the range of the equivalent forwards
> drag; anchor and focus are not distinguished downstream.

> [spec:mw:req:iced.selection.press-arbitration]
> Press handling is split between the inner widgets and the group
> wrapper. A `SelectableRichText` handles a press only when the
> cursor is inside its own bounds, and stays silent otherwise so it
> cannot fight the wrapper. The wrapper runs after the inner
> subtree: for a press inside its bounds it seeds a pending anchor
> (idempotent — an inner widget's press at the same point recorded
> the same point), and for a press outside its bounds it clears the
> selection outright. Both wrapper branches MUST be skipped while a
> drag is in flight, so an in-progress drag is never reset by the
> wrapper.

> [spec:mw:req:iced.selection.link-press-precedence]
> A press over a span carrying a link MUST NOT start a selection:
> the widget records the pressed span index, captures the event and
> returns. On release, the link handler fires only if the pressed
> span is still the hovered span. Spans are hover-tracked only when
> a link-click handler is installed, and a change in whether any
> link is hovered requests a redraw so the hover underline and the
> pointer cursor stay in step.

## Selection geometry and clipping

> [spec:mw:def:iced.selection.clip-classification]
> Clipping the shared range against a widget's bounds classifies the
> widget as `Outside` (no overlap — also the classification when no
> selection exists at all), `Whole` (fully enclosed by the
> selection), or `Partial` with a pair of widget-local points to
> hit-test. The `Outside` test is vertical only: the selection's end
> lies above the widget's top, or its start lies below the widget's
> bottom. Horizontal extent never excludes a widget, because a
> selection spanning several blocks is a text range, not a rectangle.

> [spec:mw:req:iced.selection.clip-whole]
> A widget MUST be classified `Whole` only when the selection starts
> strictly above its top *and* ends strictly below its bottom, and a
> `Whole` widget MUST contribute its entire `flat_text` without
> hit-testing (contributing nothing at all when that text is empty).
> Bypassing the hit-test here is deliberate: a hit-test at the
> bottom-right corner of a multi-line paragraph does not reliably
> resolve to the trailing offset, which would silently truncate the
> interior blocks of a multi-block drag.

> [spec:mw:req:iced.selection.clip-partial-local]
> For a `Partial` widget the two window points MUST be converted to
> widget-local coordinates by subtracting the bounds origin. A
> selection that starts above the widget contributes the local
> origin `(0, 0)` as its start; one that ends below contributes the
> bottom-right corner `(width, height)` as its end. Otherwise the
> start is floored at zero on both axes while the end is clamped
> into the widget box on both axes — the start is deliberately not
> clamped at the far edge, leaving an over-wide start x to resolve
> through the hit-test's past-the-last-glyph rule.

## Hit-testing a point to a byte offset

> [spec:mw:req:iced.hit-test.global-byte-offset]
> Hit-testing MUST resolve a widget-local point to a
> flat-text-global byte offset by downcasting the paragraph to
> `iced_graphics::text::Paragraph`, hitting its cosmic-text buffer
> to obtain a full cursor (line index plus line-local byte index),
> and adding `line_offsets[line]`. iced's own `Hit::CharOffset`
> discards the line index, so consuming it directly makes every
> multi-line widget — code blocks above all — select a rectangular
> region instead of a contiguous run of text.

> [spec:mw:sem:iced.hit-test.half-advance]
> Within a glyph the resolved offset is decided by which half of the
> grapheme cluster the point falls in: a point in the leading half
> resolves to the offset before the cluster, a point in the trailing
> half to the offset after it, with the two halves swapped for
> glyphs at an RTL bidi level. Clusters are subdivided by dividing
> the glyph's advance evenly among its graphemes. This is
> cosmic-text's rule, applied unchanged — the widget performs no
> rounding of its own.

> [spec:mw:sem:iced.hit-test.outside-glyphs]
> Points outside every glyph still resolve, so drags into margins
> and gutters behave. A point above the first visual line resolves
> to offset 0 of that line; a point left of the first glyph of an
> LTR line resolves to the start of that line; a point right of
> every glyph on a line resolves to the greatest glyph end on that
> line, which is the logical end of the line's byte coverage under
> LTR, RTL and mixed-bidi layout alike; and a point below the last
> visual line resolves to the end of the last line. A point in the
> vertical gutter between two blocks belongs to no widget's glyphs
> and is resolved by clipping, not by hit-testing: the blocks above
> it clip as `Whole` or `Partial`, the blocks below as `Outside`.

> [spec:mw:req:iced.hit-test.degenerate-range]
> When the two hit-tests of a `Partial` widget resolve to the same
> byte offset, the widget MUST contribute neither a highlight
> rectangle nor a selection fragment. A zero-width selection is not
> a selection, and an empty fragment would inject a spurious
> separator into the joined copy string.

> [spec:mw:req:iced.hit-test.paragraph-downcast-fallback]
> The concrete paragraph type is an open trait, so every path
> reaching into cosmic-text MUST degrade rather than panic when the
> downcast fails: hit-testing falls back to iced's own hit-test
> cursor (line-local, and therefore wrong for multi-line content),
> while selection painting and inline-atom painting become no-ops.
> No highlight is a better failure than a wrong one.

## Copy — serialising a selection back to text

> [spec:mw:def:iced.copy.fragment]
> During an `operate` traversal each selectable widget publishes a
> `SelectionFragment` — its bounds' top y coordinate plus its
> selected substring — through `Operation::custom`. The fragment
> type is deliberately non-generic, because `Operation::custom` sees
> the state only as `&mut dyn Any`.

> [spec:mw:def:iced.copy.selected-text-task]
> `selected_text()` is the public entry point: a widget-operation
> `Task` resolving to `Option<String>`, which the host wires to its
> copy shortcut and pipes into the clipboard. It is the only
> supported way to read the current selection; the `Selection`
> handle itself is internal.

> [spec:mw:req:iced.copy.offset-interpretation]
> cosmic-text's cursor index is a byte offset within its buffer
> line, but iced surfaces it as a char offset. Before slicing, the
> extractor MUST order the two offsets and clamp both to
> `flat_text.len()`, then use them as byte offsets when both land on
> char boundaries, and otherwise re-resolve them as char indices
> through `char_indices`, saturating at the end of the text. Drag
> direction therefore never affects the result, and a mid-codepoint
> offset never panics the slice.

> [spec:mw:req:iced.copy.atom-substitution]
> When the selected range overlaps a `Replaced` atom's placeholder
> run, the copied string MUST contain that atom's `copy_text` — the
> canonical source, e.g. the original LaTeX — in place of the
> placeholder bytes, emitted exactly once even when only part of the
> placeholder run is selected. Overlapping atoms are located from
> `span_offsets` plus placeholder length, filtered by half-open
> overlap against the selected range, sorted by start offset, and
> interleaved with the literal text lying between them. The
> placeholder characters themselves MUST never reach the clipboard.

> [spec:mw:req:iced.copy.document-order-join]
> `selected_text()` MUST collect fragments by descending into every
> widget without short-circuiting or freezing a partial outcome —
> fragments come from leaf widgets scattered anywhere in the tree,
> and any early exit silently truncates the copy. The collected
> fragments MUST be sorted by their widget's top y coordinate
> ascending and joined with a single `\n`; the sort is stable, so
> widgets sharing a top edge — side-by-side table cells, a list
> marker beside its item — keep widget-tree order. Blocks in the
> copied text are therefore separated by exactly one newline, with
> no blank line and no per-block prefix, and a traversal that
> collected no fragment at all yields `None` rather than an empty
> string.

> [spec:mw:sem:iced.copy.non-selectable-content]
> Only rich-text widgets publish fragments. Content the renderer
> emits as some other widget — raster and SVG images, successfully
> rendered pikchr / graphviz / typst diagrams, thematic-break rules,
> task-list checkboxes, list bullets and numbers, footnote number
> prefixes — contributes no text to a copy and receives no
> highlight, even when the selection spans across it. Code blocks,
> alert titles and diagram error banners are rich text and do
> participate, code blocks including their interior newlines.

## Painting the selection

> [spec:mw:req:iced.paint.layer-order]
> A widget's draw MUST paint in the order: span decorations
> (highlight backgrounds, underlines, strikethroughs, the
> hovered-link underline), then the selection highlight, then the
> glyphs, then inline-atom SVGs. The selection colour is translucent
> (`rgba(0.40, 0.60, 1.00, 0.35)`) and painted above span
> backgrounds so an inline-code background cannot hide it, and below
> the glyphs so selected text stays readable. A widget whose bounds
> do not intersect the viewport draws nothing at all.

> [spec:mw:req:iced.paint.bidi-runs]
> The highlight MUST be painted per visual line as one rectangle per
> *visually contiguous* run of selected glyphs, each rectangle
> taking the line's top and height and the run's horizontal extent,
> with glyph byte ranges shifted by their line's offset before being
> compared against the selection. A mixed-bidi line therefore yields
> several rectangles with gaps between them, matching what
> `Range::getClientRects()` produces in browsers; one rectangle
> spanning first to last selected glyph would highlight text that is
> not selected.

> [spec:mw:sem:iced.paint.glyph-run-grouping]
> A glyph counts as selected when its byte range half-open overlaps
> the selection (`glyph_start < hi && glyph_end > lo`), so an empty
> range selects no glyph and paints nothing. Selected glyphs are
> walked in visual order and coalesced into one run while the
> horizontal gap between successive glyph boxes stays within
> `ADJACENCY_EPSILON_PX` (0.5), so sub-pixel layout and kerning do
> not shatter a run into hairline rectangles; a larger gap closes
> the current run and opens a new one.

## Inline replaced atoms

> [spec:mw:req:iced.atom.placeholder-run]
> A `Replaced` atom MUST reserve its inline space with a run of
> `PLACEHOLDER_CHAR` (`'0'`), `ceil(em_width / PLACEHOLDER_EM_ADVANCE)`
> characters long with a minimum of one, coloured fully transparent
> so only the overlaid SVG is visible. The placeholder must not be
> whitespace: cosmic-text's word finder treats whitespace —
> including figure-space U+2007 — as a break opportunity, which
> would shatter the reserved space across visual lines and paint the
> SVG once per line. Over-estimating the per-character advance
> (0.6 em) is preferred, since it yields fewer placeholder
> characters and so less horizontal slack around the centred SVG.

> [spec:mw:req:iced.atom.line-box-growth]
> An atom taller than one em MUST carry a per-span line height of
> `em_height + 0.25` em, so cosmic-text — which takes the maximum
> line height across the spans on a line — grows the line it lands
> on and the atom does not touch the text above or below. Because
> cosmic-text reserves only `COSMIC_LINE_DESCENT_FRACTION` (0.22) of
> the line height as descent, while fractions, integrals and
> matrices need closer to half, the widget MUST additionally pad its
> reported minimum height by the worst-case descent overflow across
> its atoms, so that a tall atom in a tight container — a table cell
> — grows the container instead of being clipped by it.

> [spec:mw:req:iced.atom.reshape-on-bounds-change]
> A widget containing atoms MUST re-shape its paragraph from scratch
> when only its layout bounds changed, instead of taking the cheap
> resize path. Resizing re-wraps without re-deriving per-span line
> heights onto the new visual lines, so an atom that wraps onto a
> different line would keep the previous line's height.

> [spec:mw:req:iced.atom.baseline-paint]
> Atom SVGs MUST be painted once per atom per visual line, by
> walking the laid-out glyph runs and grouping consecutive glyphs
> carrying the same atom's span index into one bounding run. Each
> SVG is drawn at its natural `em_width × em_height` scaled by the
> placeholder's font size — not stretched to the placeholder run's
> measured width, which would let the aspect-preserving rasteriser
> overflow and clip the pixmap it was given — centred horizontally
> in any slack, positioned with its alphabetic baseline on the
> line's baseline less a 0.75 px optical lift, and clipped to the
> widget's bounds so it may spill past its own line.

## Renderer surface

> [spec:mw:def:iced.renderer.options]
> `IcedOptions` mirrors the parser's feature flags — `gfm`, `math`,
> `alerts`, `highlights`, `definition_lists` — plus one flag per
> compiled-in diagram engine (`pikchr`, `graphviz`, `typst`), each
> present only under its cargo feature. `IcedOptions::default()`
> enables nothing; `IcedOptions::all()` enables everything the build
> supports. A parser is built from these flags on every render.

> [spec:mw:def:iced.renderer.code-style]
> `IcedCodeStyle` carries the five colours the adapter cannot infer
> while building spans: fenced-code background, foreground and
> border, plus inline-code background and foreground. Its default is
> a light palette.

> [spec:mw:req:iced.renderer.theme-independence]
> The renderer MUST NOT consult the ambient iced `Theme` for its own
> colours: link, highlight, quote, alert and code colours are fixed
> constants or come from `IcedCodeStyle`, and the container style
> closures ignore the theme argument they are handed. A host running
> a dark or custom theme therefore has to supply colours explicitly
> through `with_code_style`. The one theme-derived value is the text
> catalog's default style, resolved per widget at draw time and used
> as the fallback glyph colour.

> [spec:mw:def:iced.renderer.url-events]
> The rendered element tree emits `Url` messages: `Url::Link` with
> the destination when a link span is clicked, and
> `Url::TaskToggle { id, checked }` when a task-list checkbox is
> toggled, where `id` is a 1-based counter assigned in document
> order across the whole render and `checked` is the *requested*
> state — the negation of the item's current state. The enum is
> `#[non_exhaustive]`; hosts lift it into their own message type
> with `Element::map`.

> [spec:mw:def:iced.renderer.custom-extension]
> `RenderIcedCustom` is the plug-in trait for rendering
> `Node::Custom` payloads into an element. The blanket
> implementation for `()` renders an empty column, so a renderer
> built without an extension drops custom nodes silently rather than
> failing.

> [spec:mw:req:iced.renderer.parse-failure-empty]
> A parse failure MUST yield an empty column rather than a panic or
> a partially built tree, so a host re-rendering on every keystroke
> of a streaming document never has to handle a render error.

> [spec:mw:sem:iced.renderer.block-widgets]
> Blocks map to widgets as follows. Paragraph and heading each
> become one rich-text widget of inline atoms, headings bold at
> 32 / 26 / 22 / 18 / 16 / 14 px for levels 1–6 and smaller levels
> clamped to the last size. A block quote becomes a tinted,
> left-padded container — a plain container rather than a row,
> because the row layout suppressed the body from the selection
> traversal. An alert becomes a container bordered in its kind's
> accent colour with a bold title row, the title defaulting to the
> kind's name. A list becomes a column of rows, each a fixed-width
> marker cell (`•`, or `N.` counting up from the list's start)
> beside the item body, spacing tighter for tight lists, with task
> items prefixed by a checkbox. A table becomes a column of rows of
> equally filled, per-column-aligned cells with a rule under the
> header. A thematic break becomes a horizontal rule, and a code
> block a rich-text widget inside a bordered, padded container.

> [spec:mw:req:iced.renderer.selection-group-wrap]
> A render MUST wrap the whole emitted column in a `SelectionGroup`
> carrying the renderer's shared `Selection`. Without the wrapper, a
> press landing between blocks reaches no widget and cannot start a
> drag.

> [spec:mw:req:iced.renderer.code-block-dispatch]
> Fenced code blocks MUST be dispatched in this order: registered
> code-block rewriters first, the first one returning a rewrite
> substituting its info string and literal for the rest of the
> pipeline; then the diagram engines, each gated on both its cargo
> feature and its option flag and matched against the first
> whitespace-delimited word of the info string (`pikchr`, `typst`,
> `dot` or `graphviz`); then the syntax highlighter for any
> non-empty info word; then plain monospace text. An unclosed fence
> has its info word cleared before dispatch, so a diagram engine is
> never run on the partial source of a block still being streamed. A
> diagram engine that fails renders an inline error banner naming
> the engine, with ANSI escapes stripped from the message, above the
> original source; a highlighter that fails falls through to plain
> text.

> [spec:mw:req:iced.renderer.render-cache]
> Diagram, math and syntax-highlight results MUST be memoised by a
> hash of their kind and source, and failures MUST be cached
> alongside successes. A host's view function runs on every message,
> so an uncached renderer would re-run pikchr, graphviz, typst, the
> math engine or the tokenizer on every scroll tick — and would
> re-run them only to fail identically each time.

> [spec:mw:req:iced.renderer.link-safety]
> A link destination MUST be checked against the URL allowlist
> before it is attached to a span. An unrecognised or dangerous
> scheme still renders its text with link colour and underline, but
> carries no href, so it is inert on click; the destination is not
> rewritten or escaped, it is simply not wired up.

> [spec:mw:req:iced.renderer.image-policy]
> The adapter performs no network access, so a block-level image
> resolves as: destination rejected by the URL allowlist — which
> includes every `data:` URI — degrades to alt text; a destination
> with no `://` separator is treated as a local filesystem path and
> loaded from there; anything remaining, i.e. a remote URL, degrades
> to alt text. Inline images take a separate path that promotes only
> `data:image/svg+xml` payloads (percent-decoded or base64) whose
> SVG declares explicit `width` and `height` within its first 512
> bytes into a `Replaced` atom, and degrades every other inline
> image to its alt text.
