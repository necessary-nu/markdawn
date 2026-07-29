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

> [spec:mw:req:editing.document.byte-offsets]
> Every offset in the `EditableDocument` API — `edit`'s range, `set_cursor`'s
> argument, `block_at`'s argument — is a byte offset into the *current*
> source buffer. `edit` splices with `String::replace_range`, so a range that
> is out of bounds or not on a UTF-8 character boundary panics; well-formedness
> of the range is a caller precondition, not an `Err`. The re-parse anchor is
> `range.start` captured before the splice, so it is an offset into the
> pre-edit buffer and is unaffected by the replacement's length. The `Err`
> variant is reserved for parse failure, principally the parser's configured
> input byte cap. Spans align with the buffer the caller indexes only when
> that buffer is already LF-normalised and NUL-free: the parser normalises
> CRLF and lone CR to LF and NUL to U+FFFD before computing spans, and the
> document keeps the un-normalised text, so a buffer containing those bytes
> yields spans that under-report against it.

## Incremental re-parse

> [spec:mw:req:editing.reparse.window]
> A re-parse anchored at byte offset `a` MUST reuse every leading block whose
> source span ends at or before `a`, and MUST re-parse everything from there
> to the end of the document. The split index is the first block whose span
> `end` is strictly greater than `a`; the re-parse input is the source suffix
> starting at the preceding block's span end, parsed with that byte position
> as the base offset so the produced spans are valid in whole-document
> coordinates. Only a prefix is windowed — the tail is always re-parsed in
> full, so the cost of an edit is linear in the distance from the edit to the
> end of the document, not in the size of the block that was edited.
> `set_source` is the degenerate case: it replaces the whole buffer and
> anchors at `0`, so the window is the entire document. Because the split
> comparison is strict, an edit whose start equals a block's span end falls
> *after* that block; text appended at the end of the buffer is therefore
> parsed as a fresh trailing block rather than extending the final one. When
> the parser carries an input byte cap, the cap is applied to the re-parsed
> suffix rather than the whole buffer, so a windowed re-parse can succeed on
> a document whose total length exceeds the cap.

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

> [spec:mw:sem:editing.cursor.reparse-edit-interaction]
> The cursor is held as a bare `CorrelationId`, not as a byte offset, and
> neither `edit` nor `set_source` remaps, revalidates, or clears it. A
> re-parse that changes the cursored block emits that block's *formatted*
> node, silently replacing the raw rendering while the cursor id still names
> the block; and because `set_cursor` short-circuits when the resolved id is
> unchanged, re-issuing `set_cursor` at the same position afterwards emits
> nothing and the block stays formatted. A caller that wants the raw
> rendering restored after an edit has to clear the cursor with
> `set_cursor(None)` and set it again. If the cursored block was deleted by
> the re-parse, the id dangles until the next `set_cursor` resolves
> elsewhere, at which point the restore emission is skipped.

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

> [spec:mw:sem:editing.serialize.block-forms]
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
> `CodeBlock` always uses a three-backtick fence with the info string
> verbatim on the opening line and a newline forced after the literal;
> `MathBlock` uses `$$` fences the same way. `ThematicBreak` is always `---`.
> `Html` is written verbatim with a newline forced. `Table` writes the first
> header row, then a delimiter row built from `alignments` (`:---`, `:---:`,
> `---:`, `---`), then the body rows, each cell padded with one space inside
> its pipes; rows that are not `TableRow` and cells that are not `TableCell`
> contribute nothing, and extra header rows are dropped. `DefinitionList`
> writes children with no separator, a term as its inlines on one line and a
> description as `: ` plus a two-space continuation indent. A variant the
> block writer does not match falls through to the inline writer, and one the
> inline writer does not match falls back to the block writer; `TableRow`,
> `TableCell`, and `Custom` are matched by neither and are reachable only
> through a parent, so serialising one of them directly recurses between the
> two fallbacks without terminating.

> [spec:mw:sem:editing.serialize.inline-forms]
> Inline emission per variant. `Text` verbatim; `Emphasis` as `*x*`; `Strong`
> as `**x**`; `Strikethrough` as `~~x~~`; `Highlight` as `==x==`; `Subscript`
> and `Superscript` as `<sub>` / `<sup>` HTML element pairs rather than any
> markdown shorthand; `Link` as `[text](dest)` with ` "title"` appended
> inside the parentheses when a title is present; `Image` as `![alt](dest)`
> with the same optional title, using the stored `alt` string rather than
> child nodes; `CodeSpan` between single backticks; `MathInline` between
> single `$`; `Html` verbatim; `SoftBreak` as a bare newline; `HardBreak` as
> two spaces then a newline; `FootnoteRef` as `[^label]`, dropping its
> resolved `number` and `ref_num`.

## Round-trip fidelity

> [spec:mw:req:editing.serialize.round-trip]
> Serialisation is canonicalising, not source-preserving: callers MUST NOT
> assume `serialize_blocks(parse(src))` equals `src`. Normalisation is
> permitted, and applied, in marker choice, indentation, inter-block spacing,
> and list numbering — headings always ATX, emphasis always asterisks,
> bullets always `-`, ordered markers renumbered consecutively from `start`,
> code always fenced rather than indented, block quotes re-prefixed line by
> line, continuation lines re-indented to the marker's width, and blocks
> re-separated by exactly one blank line. The weaker direction — re-parsing
> the serialised text reproducing the same tree — holds only up to the
> following, each of which is a real loss rather than a theoretical one:
> `SourceSpan` values are recomputed against the serialised text and will not
> equal the originals, so `Node` equality (which includes `source`) does not
> hold and only the span-erased shape can round-trip; `List::tight` is
> dropped and every list is emitted loose, so a tight list comes back loose;
> `FootnoteDefinition`'s `label`, `number`, and `backref_count` are dropped
> and its children emitted as bare blocks, so a footnote definition comes
> back as ordinary prose with any `[^label]` reference left dangling;
> `TaskItem` always emits a bullet marker, so a task item inside an ordered
> list comes back unordered; `Subscript` and `Superscript` come back as
> `Html`; `TableCell`'s `header` and `alignment` are dropped, alignment being
> reconstructed only from the table's `alignments` list; and heading levels
> are clamped to six. Fidelity is also conditional on the re-parsing
> `Parser` having the same extensions enabled as the one that produced the
> tree — with alerts off `> [!NOTE]` comes back as a plain block quote, with
> GFM off `~~x~~` comes back as literal text, and so on.

> [spec:mw:req:editing.serialize.escaping]
> There is no escaping discipline: every string payload is written verbatim.
> `Text` content is emitted unescaped, so text containing an asterisk,
> underscore, backtick, bracket, tilde, pipe, dollar, angle bracket, or a
> leading hash or hyphen re-parses as markup rather than as text. Delimiters
> are never widened or chosen defensively either: a `CodeSpan` whose code
> contains a backtick is still emitted between single backticks, a
> `CodeBlock` whose literal contains a fence line is still emitted inside a
> three-backtick fence, a link or image title containing a double quote is
> still emitted inside double quotes, a link destination containing spaces or
> parentheses is emitted bare rather than angle-wrapped, an info string is
> emitted verbatim onto the fence line, and a table cell containing a pipe is
> emitted unescaped and re-parses as extra cells. A caller needing a faithful
> round trip MUST therefore restrict itself to trees whose string payloads
> contain no markup-significant characters, or escape them before building
> the tree.
