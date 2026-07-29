# Core data model

The types every markdawn adapter is written against: the `Node<Ext>` tree,
its source spans, the `StreamItem` streaming protocol, the `Sink` and
`Extension` traits, the HTML pass-through policy, and the error taxonomy.

## The node tree

> [spec:mw:def:core.node.generic-tree]
> `Node<Ext>` is a single recursive enum covering both block and inline
> constructs; there is no separate block/inline type. It is parameterised
> by an extension payload type `Ext` that defaults to `()`. The 35
> variants are `Document`, `Paragraph`, `Heading`, `BlockQuote`, `List`,
> `Item`, `TaskItem`, `Table`, `TableRow`, `TableCell`, `Text`,
> `Emphasis`, `Strong`, `Strikethrough`, `Subscript`, `Superscript`,
> `Highlight`, `Link`, `Image`, `CodeSpan`, `CodeBlock`, `MathInline`,
> `MathBlock`, `Html`, `SoftBreak`, `HardBreak`, `ThematicBreak`,
> `FootnoteRef`, `FootnoteDefinition`, `FootnoteSection`, `Alert`,
> `DefinitionList`, `DefinitionTerm`, `DefinitionDescription` and
> `Custom`. Children are held as `Vec<Node<Ext>>` (`Table` splits them
> into `header` and `rows`; `TableRow` calls them `cells`). The enum
> derives `Debug + Clone + PartialEq + Eq`, so `Ext` must be
> `Clone + PartialEq` for any streaming or reconciliation use.

> [spec:mw:def:core.node.custom-payload]
> `Node::Custom { payload: Ext, children, source }` is the sole variant
> carrying the `Ext` payload and is the only way an extension injects a
> node the core enum does not model. It is a container: `children` holds
> ordinary `Node<Ext>` values, so an extension may attach a fully-parsed
> inline or block subtree beside its opaque payload. Adapters render it
> through a per-adapter custom-renderer plug-in, and what happens when no
> plug-in is registered is adapter-dependent rather than uniform. The
> blanket `()` implementation fails in the html, vdom, ansi, typst and
> UIKit adapters — each returning its own `UnsupportedCustomNode` error —
> so a custom node reaching one of those without a renderer aborts the
> render. The iced and Dioxus adapters instead degrade to empty output
> (an empty column and an empty `rsx!` fragment respectively), silently
> dropping the node so that a host re-rendering on every keystroke never
> has to handle a render error. Extensions that must not be dropped are
> therefore obliged to register a plug-in on the iced and Dioxus paths;
> on the other five the missing registration surfaces itself.

> [spec:mw:req:core.node.source-accessors]
> Every `Node` variant MUST carry a `source: SourceSpan` field.
> `Node::source()` and `Node::source_mut()` are total over the enum —
> they match every variant exhaustively and never return a placeholder —
> so adding a variant without a `source` field is a compile error rather
> than a silent hole in source mapping.

> [spec:mw:def:core.node.table-shape]
> A table is three nested variants. `Table { alignments, header, rows }`
> holds one `Option<TableAlignment>` per column in `alignments`, the
> header as a `Vec` of `TableRow` (currently exactly one), and the body
> as a `Vec` of `TableRow`. `TableRow { cells }` holds `TableCell`
> values. `TableCell { children, header, alignment }` holds the cell's
> inline content plus its own copy of the column alignment and a flag
> marking it as a header cell. `TableAlignment` is `Left`, `Center` or
> `Right`; `None` means the column declared no alignment and the renderer
> uses its default.

> [spec:mw:req:core.node.table-cell-alignment]
> Every row of a constructed `Table` MUST have exactly `alignments.len()`
> cells, but the header row and the body rows reach that shape by
> opposite routes. The header row MUST already match the delimiter row's
> column count: when it does not, the construct is not a table at all and
> the parser MUST reject the whole thing, leaving the lines to be
> reparsed as ordinary block content rather than padding or truncating
> the header into shape. Body rows MUST be normalised rather than
> rejected — a short row is padded with empty cells and a long row
> truncated to `alignments.len()` — so a ragged body never desynchronises
> the columns. Each `TableCell.alignment` MUST be the `alignments` entry
> at that cell's column index, and `TableCell.header` MUST be true for
> cells of the header row and false for every body cell. Renderers
> therefore read alignment off the cell and never need to walk back up to
> the `Table`.

> [spec:mw:def:core.node.alert-kinds]
> `AlertKind` enumerates the five GitHub alert kinds — `Note`, `Tip`,
> `Important`, `Warning`, `Caution` — carried by `Node::Alert` alongside
> an optional custom `title` taken from `> [!KIND] Title`.

> [spec:mw:req:core.node.alert-title-fallback]
> When `Node::Alert.title` is `None`, a renderer MUST substitute its own
> default label for the kind (`Note`, `Tip`, `Important`, `Warning`,
> `Caution`). When it is `Some`, the custom title MUST replace the
> default label entirely rather than being appended to it.

> [spec:mw:def:core.node.footnote-triple]
> Footnotes are three variants. `FootnoteRef { label, number, ref_num }`
> is the in-text marker: `label` is the un-normalized source label,
> `number` the footnote's position in the rendered list, `ref_num` which
> occurrence of that label this is. `FootnoteDefinition { label, number,
> backref_count, children }` is one entry of the list. `FootnoteSection`
> is the synthesized container appended as a trailing top-level block
> holding every definition. Definitions are lifted out of the document
> body; they never remain where they were written.

> [spec:mw:req:core.node.footnote-numbering]
> `number` MUST be the 1-based index of the label in order of first
> *reference* in the document — not order of definition — and MUST be
> identical on a `FootnoteRef` and on its `FootnoteDefinition`. `ref_num`
> MUST be the 1-based occurrence index of that reference among all
> references sharing the label, in document order.
> `FootnoteDefinition.backref_count` MUST equal the total number of
> references to that label, so a renderer can emit one back-link per
> reference without re-walking the tree.

## Traversal

> [spec:mw:req:core.traversal.direct-children]
> `Node::for_each_child` MUST invoke the callback once per *direct* child
> in document order and MUST NOT recurse. Container variants yield their
> `children` (or `cells`); the eleven leaf variants — `Text`, `Image`,
> `CodeSpan`, `CodeBlock`, `MathInline`, `MathBlock`, `Html`,
> `SoftBreak`, `HardBreak`, `ThematicBreak`, `FootnoteRef` — yield
> nothing. The match is exhaustive over the enum with no wildcard arm, so
> a new variant cannot be silently omitted from traversal.

> [spec:mw:sem:core.traversal.table-child-order]
> For `Node::Table` the direct children are its rows, not its cells: all
> `header` rows are yielded first, then all `rows`. Reaching a cell takes
> a second level of traversal through the `TableRow`.

> [spec:mw:req:core.traversal.walk-preorder]
> `Node::walk` MUST visit the receiver first and then every descendant in
> pre-order document order, by recursing through `for_each_child`. It
> therefore inherits that method's ordering exactly, including the
> header-rows-then-body-rows order for tables.

> [spec:mw:req:core.traversal.mutable-mirror]
> `for_each_child_mut` and `walk_mut` MUST mirror their shared-reference
> counterparts variant for variant and in the same order. `walk_mut`
> applies the callback to a node *before* descending into it, so a
> callback that replaces or clears a node's children changes which
> descendants are subsequently visited.

## Source spans

> [spec:mw:def:core.span.half-open-bytes]
> `SourceSpan { start, end }` is a half-open `start..end` range of **byte**
> offsets — not char or grapheme indices — into the markdown source the
> node was parsed from. `len()` is `end - start` and `is_empty()` is
> `start == end`.

> [spec:mw:req:core.span.exact-extent]
> A populated span MUST cover the construct exactly. `start` points at
> the first byte of the construct *including* any leading marker (`#`,
> `>`, a code fence, a list bullet); `end` is exclusive and includes any
> trailing newline belonging to the block. An inline span includes its
> delimiters on both sides, so `**bold**` spans both pairs of asterisks
> and `` `code` `` spans both backticks.

> [spec:mw:req:core.span.sliceable]
> Whoever constructs a populated `SourceSpan` — the block and inline
> parsers, the editable document, and any extension that builds a node —
> MUST emit offsets that are in bounds for the source being parsed and on
> UTF-8 character boundaries, so that the span is usable directly as
> `&source[start..end]`. The obligation sits on the producer because
> nothing downstream re-checks it: consumers that slice the source with a
> span — editors revealing raw text, syntax highlighters,
> attributed-string builders — are entitled to slice without defending
> against a panic, so a producer that breaks the invariant crashes its
> caller rather than being caught.

> [spec:mw:req:core.span.non-inverted]
> Whoever constructs a `SourceSpan` MUST emit an `end` greater than or
> equal to `start`. The obligation likewise rests entirely on the
> constructor, because nothing validates it afterwards: `len()` is a
> `const fn` computing `end - start` on `usize` with no guard, so an
> inverted span is not a recoverable error but an underflow panic or wrap
> at the point of use.

> [spec:mw:sem:core.span.contains]
> `SourceSpan::contains(offset)` is half-open: true when
> `start <= offset < end`. An empty span contains no offset, and an
> offset landing exactly on `end` belongs to the following construct.
> Hit-testing a cursor position against a list of block spans therefore
> returns nothing for inter-block whitespace.

> [spec:mw:sem:core.span.unpopulated-is-empty+1]
> There is no "source range unknown" encoding, and a consumer MUST NOT
> read an empty span as one. The structural nodes that carry no inline
> content of their own are populated at construction like any other: a
> table row spans its source line from its first to its last
> non-whitespace byte, excluding the line terminator; a table cell spans
> the text between its delimiting pipes with the surrounding whitespace
> excluded, which is the cell's *source* and so differs from the cell's
> content string wherever a `\|` escape was resolved; and a footnote
> definition spans the lines it was written on, trailing newline
> included, wherever in the document those lines sat before the
> definition was lifted out. An empty span is therefore a construct that
> genuinely occupies no source text at a real position: a cell that is
> empty in the source, or one padded in to square up a short body row,
> sits at the point in its row where it is or would have been, and the
> synthesized `FootnoteSection` — assembled rather than parsed — sits at
> the end of the parse input, the point at which it is appended.
>
> These are the one-shot parse's guarantees, in the coordinate space of
> the source it was handed. The streaming parser computes no block spans
> at all: every block `push_str` and `finish` emit carries `0..0`, since
> only the one-shot path walks line offsets. The footnote layer is the
> single exception, computing definition and section spans on both paths.

## The streaming protocol

> [spec:mw:def:core.stream.protocol]
> `StreamItem<N>` is the wire format between a producer (the streaming
> parser, or an editable document) and a `Sink`. It is either
> `Upsert(Emission<N>)` — where `Emission { id: CorrelationId,
> provisional: bool, node: N }` carries a whole top-level block — or
> `Delete { id: CorrelationId }`. `CorrelationId` is a `u64` naming a
> top-level block slot in the rendered output. Both producers emit the
> same protocol, which is why a sink written for one works unchanged with
> the other. There is no partial-node or patch item: the unit of
> transmission is always an entire top-level block.

> [spec:mw:req:core.stream.slot-identity]
> Correlation ids MUST be assigned by *position* in the top-level block
> list: on each reparse the block at index `i` keeps the id the previous
> block at index `i` held. Ids beyond the previous list's length are
> drawn from a strictly increasing counter and MUST NOT reuse an id that
> was previously issued, including one released by a `Delete`.

> [spec:mw:req:core.stream.change-only-upsert]
> An `Upsert` MUST be emitted for a slot only when the slot is new, when
> the node differs from the one last emitted under that id, or when the
> `provisional` flag changed. Comparison is structural equality on the
> whole node — this is what makes `Ext: PartialEq` a requirement.
> Unchanged slots are silently retained, so an edit confined to one block
> produces exactly one `Upsert`.

> [spec:mw:req:core.stream.batch-ordering]
> Within one batch of events, every `Upsert` MUST precede every `Delete`,
> and `Delete`s cover exactly the trailing slots of the previous list
> that the new list no longer has. Sinks MUST apply a batch in the order
> received; reordering or coalescing across the Upsert/Delete boundary
> breaks the position-keyed slot invariant.

> [spec:mw:req:core.stream.upsert-replaces]
> A sink MUST treat `Upsert` as create-or-replace keyed on the
> correlation id, never as append: an id it has already materialised MUST
> be updated in place, preserving its position among sibling blocks. An
> id it has not seen is appended. `Delete` removes the slot for that id.

> [spec:mw:sem:core.stream.provisional]
> `Emission.provisional` marks a block parsed from input that may still
> be extended — an unterminated final line mid-stream. A provisional
> block is a real, renderable node, not a placeholder; it is superseded
> by a later `Upsert` on the same id once more input arrives. Every block
> reaches `provisional: false` when the stream is finished, and one-shot
> parses emit only non-provisional blocks. Consumers that want the
> settled document filter on `!provisional`.

## Sinks

> [spec:mw:def:core.sink.trait]
> `Sink<N>` is the single-method output trait: `emit(&mut self, item:
> StreamItem<N>) -> Result<(), Self::Error>`, with the error type chosen
> by the implementor. It is generic over the payload `N` so adapters can
> chain — a `Node`-taking sink renders and forwards to a `String`-taking
> sink.

> [spec:mw:req:core.sink.error-aborts]
> An `Err` from `emit` MUST abort the producing call immediately: the
> remaining events of that batch are not emitted, and the error is
> surfaced to the caller wrapped as `StreamError::Sink`. Producers do not
> retry, buffer, or skip past a failed emit.

> [spec:mw:def:core.sink.vec-sink]
> `VecSink<N>` is the reference sink: it appends every item to an
> internal `Vec` in arrival order, exposed via `items()` / `into_items()`,
> and its `Error` is `Infallible` so it never aborts a producer.

## Extensions

> [spec:mw:def:core.ext.trait]
> `Extension<Ext>` is the parse-time plug-in trait. It requires
> `Send + Sync` so a parser can be shared across threads, and all three
> hooks have default bodies returning `None`, so an extension implements
> only the hook it needs.

> [spec:mw:def:core.ext.match-types]
> `InlineMatch<Ext> { consumed, node }` reports a successful inline hook:
> `consumed` counts **bytes** taken from the start of the input slice the
> hook was handed. `BlockMatch<Ext> { consumed_lines, node }` reports a
> successful block hook: `consumed_lines` counts whole lines taken
> starting at the `start` index the hook was handed. Both carry the
> `Node<Ext>` to splice into the tree — typically but not necessarily a
> `Node::Custom`.

> [spec:mw:req:core.ext.inline-match-validity]
> An `InlineMatch.consumed` MUST be greater than zero, MUST NOT run past
> the end of the parser's input, and MUST land on a UTF-8 character
> boundary. A match violating any of these is rejected with
> `Error::InvalidExtensionMatch` carrying the current byte offset — the
> parser MUST NOT clamp, truncate, or skip a bad match, because doing so
> would silently corrupt every downstream span.

> [spec:mw:req:core.ext.dispatch-order]
> Extensions MUST be consulted before the built-in syntax at each parse
> position — inline hooks first in the inline loop, block hooks first in
> the block loop — and MUST be tried in registration order, with the
> first extension returning `Some` winning. An extension can therefore
> shadow built-in markdown syntax, and an earlier registration wins over
> a later one.

> [spec:mw:req:core.ext.ctx-delegation]
> The parser MUST call `try_inline_ctx`, never `try_inline`, so that the
> `InlineContext` is always available. `try_inline_ctx`'s default body
> MUST delegate to `try_inline`, so an extension that overrides only
> `try_inline` keeps working unchanged.

> [spec:mw:def:core.ext.inline-context]
> `InlineContext<Ext>` is the narrow re-entrancy surface handed to
> `try_inline_ctx`. Its single method `parse_inline(&self, source: &str)
> -> Vec<Node<Ext>>` runs the full inline parser — emphasis, code spans,
> links, and every other registered extension — over a substring the
> extension captured, so a construct like `:role[text](target)` can treat
> `text` as markdown rather than as a literal string.

> [spec:mw:sem:core.ext.inline-context-isolation]
> `parse_inline` treats its input as a self-contained document fragment:
> link reference definitions and footnote labels from the surrounding
> document are *not* inherited, so a `[foo]` reference inside a captured
> substring will not resolve. The nested parse is never provisional, and
> a parse error yields an empty node list rather than propagating.

> [spec:mw:req:core.ext.block-zero-consumed]
> A `BlockMatch` with `consumed_lines == 0` MUST be discarded and the
> remaining extensions tried — unlike the inline path, a zero-length
> block match is not an error, just a non-match. This keeps a
> zero-consuming extension from wedging the block loop.

## Code-block rewriters

> [spec:mw:def:core.rewriter.types]
> `CodeBlockRewriter` is the render-time plug-in for fenced code blocks:
> `rewrite(&self, info: Option<&str>, literal: &str) ->
> Option<CodeBlockRewrite>`, where `CodeBlockRewrite { info, literal }`
> is the replacement pair. Like `Extension` it is `Send + Sync`. It
> generalises the built-in `info == Some("pikchr")` dispatch so
> downstream crates can register their own info-string handlers.

> [spec:mw:req:core.rewriter.render-time-only]
> A rewrite MUST affect rendering only: the parsed `Node::CodeBlock`
> keeps its original `info` and `literal`. Serializing the tree back to
> markdown therefore reproduces the source as written, and re-rendering
> the same tree re-runs the rewriters.

> [spec:mw:req:core.rewriter.dispatch-order]
> Rewriters MUST run before the built-in code-block dispatch, in
> registration order, with the first `Some` winning and the rest skipped.
> The rewritten `info` and `literal` MUST then flow through the *entire*
> remaining pipeline rather than short-circuiting it: a rewritten
> diagram-language info reaches the diagram engine, a rewritten language
> tag reaches the syntax highlighter, and an unrecognised one falls
> through to plain output. When no rewriter matches, the original values
> are used unchanged.

## HTML pass-through

> [spec:mw:def:core.html-filter.modes]
> `HtmlFilter` is the policy an adapter applies to every `Node::Html`
> before writing it out. It has two modes: sanitize against a
> `SanitizerConfig` (`HtmlFilter::safe()` for the W3C-aligned defaults,
> `HtmlFilter::custom(config)` for a caller-supplied one) and
> `HtmlFilter::unrestricted()` for verbatim pass-through, distinguishable
> at runtime via `is_unrestricted()`. Filtering runs a real HTML parser,
> not a byte-level tag scan, so multi-byte content spanning a scan window
> cannot cause a panic. The type is cheap to clone.

> [spec:mw:req:core.html-filter.default-safe]
> `HtmlFilter::default()` MUST be `safe()`, and every adapter that
> accepts a filter MUST default to it. Verbatim HTML is opt-in only:
> reaching `unrestricted()` requires an explicit call, so a caller who
> configures nothing gets sanitized output.

> [spec:mw:req:core.html-filter.fail-closed]
> When sanitization fails, `filter` MUST return the empty string —
> dropping the input — rather than returning it unchanged. The failure
> mode is loss of content, never leakage of unsanitized markup.

## Errors

> [spec:mw:def:core.error.taxonomy]
> `Error` has four variants covering everything the parser can refuse.
> `AlreadyFinished`: input pushed after the stream was finished.
> `InvalidExtensionMatch`: an extension returned an out-of-range or
> non-boundary inline match. `MalformedEventStream`: the inline tokenizer
> reached a state the parser considers impossible. `InputTooLarge { seen,
> cap }`: cumulative input exceeded a configured byte cap — raised only
> when the caller opted in, since the default is unlimited. The first
> three are `Option<ErrorLocation>`-carrying; none of them are
> recoverable mid-parse.

> [spec:mw:sem:core.error.location]
> `ErrorLocation.byte_offset` is an offset into the parser's *normalized*
> buffer — after line-ending and tab normalization — not into the bytes
> the caller pushed, so it is not in general a valid index into the
> caller's original input. It is optional because not every failure site
> can attribute a position.

> [spec:mw:def:core.error.stream-error]
> `StreamError<E>` separates the two failure sources of a sink-driven
> parse: `Parser(Error)` for a parse refusal and `Sink(E)` for the sink's
> own error type. `From<Error>` exists so parser failures lift
> implicitly, but there is deliberately no blanket `From<E>` — sink
> errors are wrapped explicitly at the emit site. `Display` and
> `std::error::Error` forward to whichever side failed.

## In-document URLs

> [spec:mw:def:core.url.scheme]
> markdawn renders some inline constructs as tappable links under a
> private `markdawn:` URL scheme so a host application can intercept them
> instead of opening a browser. `MarkdawnUrl` enumerates the navigation
> targets; today the only one is `Footnote { label }`, encoded as
> `markdawn:footnote/<label>`, where `label` is the original
> un-normalized footnote label from the source.

> [spec:mw:req:core.url.parse-strictness]
> `parse_markdawn_url` MUST return `Some` only for a URL carrying the
> `markdawn:` scheme *and* a recognised target path. Anything else —
> `http`/`https` links, and `markdawn:` URLs with an unknown path — MUST
> return `None` so the caller hands it to the platform's default URL
> handling rather than swallowing the interaction.
