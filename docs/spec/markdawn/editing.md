# Editing

Incremental re-parse of a live markdown buffer (`src/editor.rs`) and
rendering of a `Node` tree back to markdown source (`src/serializer.rs`).

## Live document

> [spec:mw:def:editing.document.editable-document]
> An `EditableDocument<Ext>` is a live markdown buffer: an owned source
> `String`, an ordered list of top-level `LiveBlock<Ext>` (each pairing a
> `CorrelationId` with a parsed `Node<Ext>` and its `provisional` flag), a
> configured `Parser<Ext>`, a monotonically increasing id counter seeded at
> `1`, and an optional cursor block id. Mutating calls return
> `Vec<StreamItem<Node<Ext>>>` — the same `Upsert` / `Delete` protocol the
> streaming parser emits — so a sink written against the streaming parser
> consumes editor events unchanged. Only top-level blocks are tracked: the
> document exposes no identity for nodes nested inside a block. The parser's
> streaming state is reset on every re-parse; only its configuration
> (extensions, GFM / math / alert / highlight / definition-list flags, input
> byte cap) is retained across edits.

> [spec:mw:req:editing.document.byte-offsets+1]
> Every offset in the `EditableDocument` API — `edit`'s range, `set_cursor`'s
> argument, `block_at`'s argument — is a byte offset into the *current*
> source buffer. `edit` splices with `String::replace_range`, so a range that
> is out of bounds or not on a UTF-8 character boundary panics; well-formedness
> of the range is a caller precondition, not an `Err`. The re-parse anchor is
> `range.start` captured before the splice, so it is an offset into the
> pre-edit buffer and is unaffected by the replacement's length. The `Err`
> variant is reserved for parse failure, principally the parser's configured
> input byte cap.
>
> The buffer is normalised on ingest, because the parser computes spans
> against normalised text: `set_source` stores the normalisation of its
> argument, and `edit` renormalises the spliced buffer, so CRLF and lone CR
> become LF and NUL becomes U+FFFD in the stored text. "Spans index the
> stored buffer" therefore holds by construction rather than by the caller's
> luck. The document consequently does not echo the caller's exact bytes: a
> CRLF document reads back from `source()` LF-normalised, and its byte length
> shrinks by one per line ending, while a NUL grows to three bytes. Callers
> MUST compute the offsets they pass in — and interpret the spans they are
> handed — against that normalised text, not against the original they
> supplied; a caller that needs its own coordinates MUST normalise before it
> starts and index the normalised copy.

## Incremental re-parse

> [spec:mw:req:editing.reparse.window+1]
> A re-parse anchored at byte offset `a` MUST reuse every leading block whose
> source span ends strictly before `a`, and MUST re-parse everything from
> there to the end of the document. The split index is the first block whose
> span `end` is greater than or equal to `a`; the re-parse input is the source
> suffix starting at the preceding block's span end, parsed with that byte
> position as the base offset so the produced spans are valid in
> whole-document coordinates. Only a prefix is windowed — the tail is always
> re-parsed in full, so the cost of an edit is linear in the distance from the
> edit to the end of the document, not in the size of the block that was
> edited. `set_source` is the degenerate case: it replaces the whole buffer
> and anchors at `0`, so the window is the entire document. The comparison
> MUST be inclusive, because a block's span `end` is the offset text spliced
> in there extends it *from*: an edit anchored exactly at the final block's
> end is an append to that block, and one anchored at an interior block's end
> lands on the newline the block owns. Excluding such a block from the window
> leaves it parsed against text that no longer follows it, splitting an
> appended keystroke off into a spurious trailing block. When the parser
> carries an input byte cap, the cap is applied to the re-parsed suffix rather
> than the whole buffer, so a windowed re-parse can succeed on a document
> whose total length exceeds the cap.

> [spec:mw:req:editing.reparse.correlation-stability]
> Correlation ids MUST survive a re-parse. Reused prefix blocks keep their
> ids untouched and emit no event at all. Within the re-parsed window the
> i-th new block inherits the id of the i-th old block of that window; a
> fresh id is minted from the counter only for positions past the end of the
> old window, and ids are never reused after a delete. An `Upsert` is emitted
> for a window position only when the new node or its `provisional` flag
> differs from the old one, so positions that re-parse identically stay
> silent — this is what makes the protocol incremental, and a re-parse that
> minted new ids for unchanged blocks would destroy it. When the window
> shrinks, one `Delete { id }` per surplus trailing old block is emitted
> after all upserts, in document order. Identity is positional, not
> content-derived: inserting or removing a block inside the window shifts
> every following block onto its predecessor's id, so a downstream consumer
> observes the window's tail renumbered and its last ids deleted rather than
> a single structural insertion or removal.

## Cursor reveal

> [spec:mw:req:editing.cursor.reveal-source]
> `set_cursor(Some(offset))` resolves the offset through `block_at` and, when
> the resolved block id differs from the current one, MUST emit at most two
> `Upsert`s in this order: first the previously-cursored block re-emitted
> under its own id with its stored formatted node, then the newly-cursored
> block re-emitted under its own id as a `Node::CodeBlock` whose `info` is
> `"markdown"` and whose `literal` is the buffer slice of the block's span
> with trailing newlines stripped. Both emissions carry the block's
> `provisional` flag, and the synthesised `CodeBlock` keeps the block's
> original span, so the swap between formatted and raw rendering is an
> in-place content replacement for any consumer keyed by correlation id. When
> the resolved id is unchanged — including moving within the same block, and
> including `None` to `None` — no events are emitted at all. A
> previously-cursored id no longer present in the block list produces no
> restore event. `set_cursor` never re-parses, never mutates the buffer, and
> cannot fail.

> [spec:mw:sem:editing.cursor.reparse-edit-interaction+1]
> A re-parse ends the reveal. The cursor is held as a bare `CorrelationId`
> and ids are *positional*, so a re-parse that inserts or removes a block
> ahead of the cursored one leaves that id naming different content; carrying
> the cursor across would re-reveal the wrong block, and remapping it is not
> possible from an id alone. `edit` and `set_source` MUST therefore clear the
> cursor. Clearing it is not sufficient on its own: the re-parse emits an
> `Upsert` only for slots whose node changed, so a cursored block that
> re-parsed identically would keep the raw rendering the consumer was last
> given, with nothing left to restore it. When a cursor was set and its block
> is still in the list, the document MUST therefore emit that block's
> formatted node under its id, unless the re-parse already emitted for that
> id, and MUST place the emission before any `Delete` in the batch so
> `[spec:mw:req:core.stream.batch-ordering]` still holds. A block the
> re-parse deleted needs no restore. The cursor position after an edit is the
> caller's to decide, so the caller re-issues `set_cursor` with the offset the
> cursor now sits at; because the document is no longer holding the old id,
> that call is not short-circuited even when it resolves to the same block.

## Block lookup

> [spec:mw:sem:editing.block-at.containment]
> `block_at(offset)` scans the top-level block list in document order and
> returns the first block whose source span contains the offset under
> half-open containment (`start <= offset < end`). It does not descend into a
> block's children, so an offset inside a list item, table cell, or block
> quote resolves to the outermost block. It returns `None` for an offset
> landing in the blank-line separator between two blocks, for an offset equal
> to the buffer length, and for any offset past the end. At a boundary the
> offset belongs to the block that starts there, never to the block that ends
> there.

## Markdown emission

> [spec:mw:def:editing.serialize.entry-points]
> `serialize(node)` renders a single node to markdown; `serialize_blocks(blocks)`
> renders a slice of top-level blocks. Every block writer terminates its
> output with a newline, and `serialize_blocks` separates adjacent blocks by
> topping the buffer up to exactly one blank line — appending newlines only
> until the buffer ends in two, so the separator is idempotent and never
> stacks. Emission carries a write context holding list-item nesting depth
> and block-quote depth, cloned when descending into a container so nesting
> is reconstructed by recursion rather than by a running indent string. The
> writers are pure functions of the node tree: no source span, correlation
> id, or parser configuration is consulted.

> [spec:mw:sem:editing.serialize.block-forms+1]
> Block emission per variant. `Document` and the two footnote containers
> write their children blank-line-separated. `Paragraph` writes its inlines
> plus a newline. `Heading` writes `min(level, 6)` hash characters, a space,
> its inlines, and a newline — never setext form. `BlockQuote` renders its
> children into a scratch buffer and prefixes every resulting line with
> `"> "`, so nesting yields `"> > "` and an interior blank line becomes a
> line of `"> "` with a trailing space. `Alert` emits `> [!KIND]` with the
> custom `title` appended on the same line when present, then its children
> under the same prefixing. `List` writes each item with a `- ` bullet or,
> when ordered, `<start + index>. ` renumbered consecutively; items are
> separated by a blank line and each item's continuation lines are indented
> by the marker's width. `TaskItem` always uses `- [x] ` or `- [ ] `.
> `CodeBlock` uses a backtick fence, or a tilde fence when the info string
> holds a backtick, sized per `editing.serialize.escaping`, with the info
> string verbatim on the opening line and a newline forced after the
> literal; `MathBlock` uses `$$` fences, never resized. `ThematicBreak` is
> always `---`. `Html` is written verbatim with a newline forced. `Table`
> writes the first header row, then a delimiter row built from `alignments`
> (`:---`, `:---:`, `---:`, `---`), then the body rows, each cell padded
> with one space inside its pipes; rows that are not `TableRow` and cells
> that are not `TableCell` contribute nothing, and extra header rows are
> dropped. `DefinitionList` writes children with no separator, a term as its
> inlines on one line and a description as `: ` plus a two-space
> continuation indent. `TableRow` reached on its own emits a bare row line,
> `TableCell` its inlines plus a newline, and `Custom` its children as
> blocks — an extension payload is opaque and contributes nothing itself.
> Every remaining variant is inline and is handed to the inline writer. Both
> writers match exhaustively, and the variants each hands to the other are
> matched there directly, so a node crosses between them at most once and
> emission always terminates.

> [spec:mw:sem:editing.serialize.inline-forms+1]
> Inline emission per variant, threaded through a state pair recording
> whether the next character starts an output line and whether emission sits
> inside a link or image label — the two positions `editing.serialize.escaping`
> keys off. `Text` is escaped per that rule; `Emphasis` as `*x*`; `Strong`
> as `**x**`; `Strikethrough` as `~~x~~`; `Highlight` as `==x==`; `Subscript`
> and `Superscript` as `<sub>` / `<sup>` HTML element pairs rather than any
> markdown shorthand; `Link` as `[text](dest)` with ` "title"` appended
> inside the parentheses when a title is present; `Image` as `![alt](dest)`
> with the same optional title, using the stored `alt` string rather than
> child nodes; `CodeSpan` between backtick runs sized to its content;
> `MathInline` between single `$`; `Html` verbatim; `SoftBreak` as a bare
> newline; `HardBreak` as two spaces then a newline; `FootnoteRef` as
> `[^label]`, dropping its resolved `number` and `ref_num`. `TableCell` and
> `Custom` write their children as inlines. Every remaining variant is
> block-level and is handed to the block writer.

## Round-trip fidelity

> [spec:mw:req:editing.serialize.round-trip+2]
> Serialisation is canonicalising, not source-preserving: callers MUST NOT
> assume `serialize_blocks(parse(src))` equals `src`. Normalisation is
> permitted, and applied, in marker choice, indentation, inter-block spacing,
> and list numbering — headings always ATX, emphasis always asterisks,
> bullets always `-`, ordered markers renumbered consecutively from `start`,
> code always fenced rather than indented, block quotes re-prefixed line by
> line, continuation lines re-indented to the marker's width, and blocks
> re-separated by exactly one blank line. Delimiters are sized and string
> payloads escaped per `editing.serialize.escaping`, so the weaker
> direction — re-parsing the serialised text reproducing the same tree —
> holds for arbitrary payload content, and only up to the following, each of
> which is a real loss rather than a theoretical one:
> `SourceSpan` values are recomputed against the serialised text and will not
> equal the originals, so `Node` equality (which includes `source`) does not
> hold and only the span-erased shape can round-trip; `List::tight` is
> dropped and every list is emitted loose, so a tight list comes back loose;
> `FootnoteDefinition`'s `label`, `number`, and `backref_count` are dropped
> and its children emitted as bare blocks, so a footnote definition comes
> back as ordinary prose with any `[^label]` reference left dangling;
> `TaskItem` always emits a bullet marker, so a task item inside an ordered
> list comes back unordered; `Subscript` and `Superscript` are written as
> `<sub>` / `<sup>` and survive only where the re-parse promotes those tags
> back, which is the top level of a block's inline run — nested inside
> another inline (emphasis, strong, strikethrough, highlight, or a link
> label) the promotion does not reach them and they come back as an `Html`
> open tag, the content, and an `Html` close tag; `TableCell`'s `header` and
> `alignment` are dropped, alignment being
> reconstructed only from the table's `alignments` list; heading levels
> are clamped to six; and the payloads `editing.serialize.escaping` leaves
> verbatim come back altered when they hold markup. Fidelity is also
> conditional on the re-parsing
> `Parser` having the same extensions enabled as the one that produced the
> tree — with alerts off `> [!NOTE]` comes back as a plain block quote, with
> GFM off `~~x~~` comes back as literal text, and so on.

> [spec:mw:req:editing.serialize.escaping+1]
> Emission MUST NOT let a string payload re-parse as structure the tree does
> not contain, and escaping is positional: a character that cannot open a
> construct where it sits is written bare, because gratuitous backslashes
> are themselves a defect. In a `Text` payload the writer always backslashes
> every character of a run of `\`, `` ` ``, `*`, or `[`; backslashes `]` only
> inside a link or image label, where an unescaped one ends the label early
> and outside of which it is inert; `_` unless the run is flanked by
> alphanumerics on both sides, which can neither open nor close emphasis;
> `<` only when the next character could begin a tag or autolink and a later
> `>` could close it; `&` only when it opens a decodable entity or numeric
> character reference; `~` in runs of one or two, which are strikethrough
> delimiters, three or more being literal; `=` in runs of two or more; and
> `$` in runs of two or more or when a later `$` could close a math span.
>
> At the start of an emitted line — the head of a block's own line, or the
> position after a `SoftBreak` or `HardBreak` — the writer additionally
> backslashes the first character of a `#` run of at most six followed by a
> space, tab, or line end; of any `>`; of a `-` or `+` followed by a space,
> tab, or line end; of a `-` run of three or more; of a `:` followed by a
> space or tab; and the `.` or `)` of a leading run of at most nine digits
> followed by a space, tab, or line end. It also backslashes a whole `~` run,
> which would otherwise open a tilde fence, and a whole `=` run filling the
> rest of its line, which would otherwise underline a setext heading. Those
> markers are inert away from a line start and are left alone there.
>
> Delimiters are sized to their content rather than fixed. A `CodeSpan` is
> fenced by the shortest backtick run its content does not itself contain,
> padded with one space on each side when the content begins or ends with a
> backtick so the opening and closing runs stay exactly that length, and its
> line endings become spaces. A `CodeBlock` fence is at least three
> characters and at least one longer than the longest closing-fence-shaped
> line in its literal, and switches from backticks to tildes when the info
> string contains a backtick, which a backtick fence's info string may not.
> A link or image title escapes `\`, `"`, `(`, `)`, and entity-opening `&`,
> and its line endings become spaces. A destination is angle-wrapped when it
> is empty, holds whitespace or an angle bracket, or has unbalanced
> parentheses; it escapes `\` and entity-opening `&` either way, and the
> angle brackets themselves when wrapped. A table cell is rendered first and
> every `|` in the result escaped afterwards — including ones inside code
> spans and link targets, which row splitting unescapes before inline
> parsing reads the cell — and interior line endings become spaces.
>
> Payloads that stay verbatim, and so still re-parse altered when they hold
> markup: a `CodeBlock` info string, whose backslash escapes and entity
> references are decoded on the way back; `MathInline` and `MathBlock`
> bodies, which a `$` in the body truncates; an `Alert` title; `Html`; and a
> `FootnoteRef` label. A `CodeSpan` whose content is empty, or begins and
> ends with a space, has no faithful markdown form and does not survive the
> trip either.
