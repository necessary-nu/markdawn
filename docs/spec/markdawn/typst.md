# typst — Typst output adapter and compilation environment

Two halves of one concern.

The **adapter** (`src/adapter/typst.rs`) is a pure tree-to-text walker:
it takes a `Node<Ext>` and emits `.typ` source. It never invokes the
compiler and has no I/O. Typst is markup-first, so some constructs map
to markup (`= heading`, `_emph_`) and others must use a function-call
form because markup cannot carry the arguments we need — an ordered
list starting at `3.` has no markup spelling, so we emit
`#enum(start: 3, ...)`. A handful of emitted calls (`#admonition`,
`#task-item`) are *not* Typst builtins; the consumer's template supplies
them.

The **compilation environment** (`src/typst/`) is the other direction:
a `typst::World` implementation with no filesystem, no network and no
clock, whose only inputs are the user's source, a compiled-in font set
and a compiled-in `@preview` package tree. It exists so a markdown
document containing a `typst` fence renders identically on any machine,
offline.

The DoS and isolation posture of the compile calls — input caps,
wall-clock timeouts, panic capture, render serialisation — is specified
separately in `docs/spec/adversarial.md` under `adv:typst.render.*` and
`adv:typst.input.*` and is not restated here.

## Adapter surface

> [spec:mw:def:typst.adapter.surface]
> `TypstAdapter<R>` walks a `Node<Ext>` tree and emits a `.typ` source
> string. `render_node` renders one node; `render_many` renders a slice
> of top-level siblings into one string; `render` maps a
> `StreamItem<Node<Ext>>` to a `StreamItem<String>`, preserving the
> emission `id` and `provisional` flag and passing `Delete` through
> untouched. `TypstSink<S, R>` wraps the adapter as a `Sink<Node<Ext>>`
> that forwards rendered strings to an inner `Sink<String>`, mapping the
> inner sink's error into `TypstError::Sink(String)`. The type parameter
> `R` is a `RenderTypstCustom<Ext>` plug-in invoked for `Node::Custom`
> payloads at both block and inline level; it is handed a `TypstWalker`
> holding borrowed closures over the adapter's own block and inline
> walkers, so a custom renderer descends into children through exactly
> the code path the adapter uses. The default `R = ()` implements the
> trait by rejecting every payload with
> `TypstError::UnsupportedCustomNode`, so a tree carrying custom nodes
> fails loudly rather than silently dropping content.

## Node-to-markup mapping

> [spec:mw:sem:typst.adapter.block-mapping]
> Block-level nodes emit as follows. `Document` concatenates its
> children. `Paragraph` emits its inlines followed by a blank line.
> `Heading` emits a leading newline, `level` (clamped to 6) `=`
> characters, a space, its inlines, then a blank line. `BlockQuote`
> becomes `#quote(block: true)[...]`. Lists always use the function
> form and never the `-` / `+` markup form, because markup cannot carry
> a start index and task items cannot round-trip through it: ordered
> lists emit `#enum(start: N, tight: false, [..], ..)` with `N` from
> `start` defaulting to 1, unordered lists emit
> `#list(tight: false, [..], ..)`, and each item's trailing newlines are
> trimmed before its closing bracket so items do not become separate
> paragraphs. A `TaskItem` reached outside a list emits
> `#task-item(done: true|false)[...]`. `CodeBlock` emits a raw block
> whose language is the first whitespace-delimited word of `info`;
> an `ansi` language has its escape sequences stripped and a `terminal`
> language is replayed through a VT emulator and flattened to its final
> visible screen, and both emit with no language tag since the residue
> is plain text. `MathBlock` emits `$ math $`. `ThematicBreak` emits
> `#line(length: 100%)`. `Alert` emits
> `#admonition(kind: "note"|"tip"|"important"|"warning"|"caution")[...]`
> with an optional `title:` argument. `DefinitionList` pairs its
> children two at a time into `#terms(tight: false, ([term], [desc]), ..)`,
> ignoring any trailing unpaired child. `Html`, `FootnoteSection`,
> `FootnoteDefinition`, and stray `TableRow` / `TableCell` /
> `DefinitionTerm` / `DefinitionDescription` at block level emit
> nothing, so an unmapped construct degrades to omission rather than to
> a document that fails to compile. A `Table` emits
> `#table(columns: N, align: (..), [cell], ..)` where `N` is the larger
> of the alignment count and the widest row (minimum 1), the `align`
> tuple is padded with `auto` up to `N` and omitted entirely when no
> alignments are given, and header and body cells are emitted as one
> flat comma-separated sequence — there is no header marker, so header
> cells are not distinguishable in the output. An inline-level leaf
> encountered at block level falls through to the inline writer.

> [spec:mw:sem:typst.adapter.inline-mapping]
> Inline nodes emit as follows. `Text` emits escaped text. `Emphasis`
> and `Strong` use the markup forms `_..._` and `*...*`; `Strikethrough`,
> `Subscript`, `Superscript` and `Highlight` use `#strike[...]`,
> `#sub[...]`, `#super[...]` and `#highlight[...]`. A `Link` whose
> destination begins with `#` and whose remainder is non-empty and
> composed only of ASCII alphanumerics, `-`, `_`, `.` and `:` is treated
> as an in-document reference and emits `#link(<label>)[...]`; every
> other destination emits `#link("dest")[...]`. An `Image` with an
> `http://` or `https://` destination is never fetched — Typst's
> `image()` cannot load remote files, and a print pipeline should not
> reach the network — and instead emits a link carrying the alt text in
> italics, or the literal `(image)` when the alt text is empty; a local
> destination emits `#figure(image("dest"))`, gaining
> `, caption: [alt]` when the alt text is non-empty. `CodeSpan` emits a
> raw inline span. `MathInline` emits `$math$` with no padding spaces,
> which is what distinguishes it from the block form. `SoftBreak`
> becomes a single space and `HardBreak` becomes a trailing backslash
> plus newline. `FootnoteRef` and inline `Html` emit nothing: without a
> definition side-channel there is nothing to place, so a consumer
> wanting real footnotes must pre-resolve refs into custom nodes
> emitting `#footnote[...]`. A block-level node encountered at inline
> level falls through to the block writer.

## Escaping

> [spec:mw:req:typst.adapter.escaping]
> Author-supplied bytes MUST be made inert for the Typst context they
> land in, and there are three such contexts. (1) In markup text, the
> characters backslash, `#`, `$`, backtick, `*`, `_`, `[`, `]`, `<`, `>`
> and `@` MUST each be emitted backslash-prefixed; newlines pass through
> unchanged and every other character is emitted verbatim — the escape
> set is exactly those eleven, so a re-implementation that adds or drops
> one is not conformant. The line-start-only markers `=`, `+`
> and `-` are deliberately NOT escaped — escaping them would corrupt
> ordinary prose containing hyphens and equals signs far more often than
> it would prevent a stray heading or list. (2) Inside a `"..."` string
> literal — link and image destinations, admonition titles — only `\`
> and `"` are escaped; markup specials are not, because they carry no
> meaning there. (3) Raw spans and raw blocks MUST NOT escape their
> content at all; instead the fence length is chosen as one more than
> the longest run of backticks in the literal, so no literal can
> terminate its own fence. Block fences additionally take a floor of
> three backticks so a block never degrades into an inline span, and a
> block inserts a newline after the opening fence when the literal does
> not begin with one and before the closing fence when it does not end
> with one. Inline spans have no floor and no padding.

## Compilation environment

> [spec:mw:def:typst.world.hermetic]
> A `RenderWorld` is the `typst::World` used for one compile pass, and
> its entire input set is closed at build time. The user's source is the
> single virtual file `main.typ` rooted at `VirtualRoot::Project`. Fonts
> are exactly those returned by `typst_assets::fonts()` — Libertinus
> Serif in six weights and styles, New Computer Modern in four text
> faces plus three math faces, and DejaVu Sans Mono in four faces —
> loaded once into a process-wide `OnceLock` alongside a
> `Library::default()` and a `FontBook` derived from them, and shared by
> every subsequent compile. Each font file is walked by face index until
> `Font::new` returns `None`, so a collection file contributes all of
> its faces rather than only the first; `World::font(i)` is a plain
> bounds-checked index into that vector and returns `None` past the end
> rather than panicking. There is no filesystem, no network, no
> environment and no real clock: `today()` returns a fixed 2024-01-01,
> which is what makes two renders of the same source byte-identical.
> Every file id that is neither `main.typ` nor a bundled package file
> resolves to `FileError::NotFound`.

> [spec:mw:req:typst.packages.offline-resolution]
> Bundled `@preview` packages MUST be resolvable with no package cache
> and no network. The tree under `assets/typst-packages` is compiled
> into the binary with `include_dir!` and mirrors the layout the Typst
> CLI uses for its package cache — `preview/{name}/{version}/{path}` —
> so a vendored release tarball drops in verbatim. A file id resolves
> only when all three hold: its root is `VirtualRoot::Package`, its
> namespace is exactly `preview`, and its name-and-version pair appears
> in the compiled-in allowlist, currently `syntree 0.2.1`, `cetz 0.5.0`
> and `oxifmt 1.0.0`. `oxifmt` is present only because `cetz` imports
> it; an indirect dependency MUST be bundled explicitly or the package
> that needs it fails to compile. Anything else — another namespace, an
> unlisted name, a version that does not match exactly — MUST return
> `FileError::NotFound` with no fallback to a real package cache and no
> network fetch. When building the lookup key only `Normal` path
> components are copied, so `..` and absolute-root components cannot
> escape the bundled tree.

> [spec:mw:req:typst.preamble.svg-auto-page]
> The SVG path MUST prepend a fixed preamble to the user's source
> setting the page to `width: auto, height: auto, margin: 0.5em,
> fill: none` and the text size to `11pt`, and MUST merge the compiled
> pages into a single SVG with four points of padding. Without this a
> diagram fence would be typeset onto a blank A4 sheet with an opaque
> background instead of sizing to its own content and compositing into
> the host page. The PDF path MUST NOT prepend it: PDF callers author
> whole documents and supply their own page setup, page numbering and
> table of contents. The preamble is hard-coded — there is no per-fence
> configuration surface — and because it becomes part of the compiled
> source, line and column numbers in SVG-path diagnostics are offset by
> its two lines.

## SVG post-processing

> [spec:mw:req:typst.svg.inline-uses]
> `typst-svg` writes each shaped glyph once into
> `<defs><symbol id="g..."><path .../></symbol></defs>` and references it
> at each occurrence with `<use xlink:href="#g..." x=".." y=".."
> fill=".." fill-rule=".."/>`. The downstream W3C-safe-baseline SVG
> sanitiser strips `<use>` wholesale, so the renderer MUST inline those
> references before returning: every `<symbol>` block is dropped and
> every `<use>` is replaced by
> `<g transform="translate(X Y)" fill=".." fill-rule="..">` wrapping a
> verbatim copy of the referenced symbol's body. Leaving this to the
> sanitiser would silently erase the entire text layer of every diagram.
> Absent `x` / `y` default to `0`; absent `fill` / `fill-rule`
> attributes are omitted rather than defaulted; and a `<use>` whose href
> names no collected symbol emits an empty `<g>` rather than failing, so
> a malformed reference costs one glyph and not the diagram. The scan
> MUST treat `<use` as a tag only when the next byte is whitespace, `/`
> or `>`, so an unrelated element such as `<user-data>` is copied
> through untouched. An emptied `<defs>` is left in place; it is inert
> and survives sanitisation. String scanning rather than XML parsing is
> sufficient here because `typst-svg` is a deterministic serialiser with
> fixed attribute order and never nests a `<use>` inside a `<symbol>`.
