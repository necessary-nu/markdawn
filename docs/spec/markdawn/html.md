# html — HTML and virtual-DOM rendering adapters

Covers `src/adapter/mod.rs`, `src/adapter/vdom.rs`, `src/adapter/html.rs`
and `src/adapter/escape.rs`.

Scope note: *which* HTML each markdown construct produces is fixed by the
GFM conformance spec (`docs/spec/spec.md`) and is not restated here. This
document specifies the adapters' own contract — the module surface, the
shape of the intermediate render tree, the string serializer, the
escaping discipline, and the custom-node hooks.

## Adapter module surface

> [spec:mw:def:html.adapter.surface]
> `adapter` groups one module per output target and re-exports each
> target's public names from the crate root. The serializing targets
> follow a four-part convention: an `<T>Adapter` that maps one
> `StreamItem<Node<Ext>>` to one `StreamItem<Output>`, an `<T>Sink` that
> implements `Sink<Node<Ext>>` by rendering and forwarding to an inner
> `Sink<Output>` (and hands the inner sink back via `into_inner`), a
> `Render<T>Custom<Ext>` extension hook, and an `<T>Error` enum whose
> `Sink(String)` variant stringifies the inner sink's error so the
> adapter's error type does not depend on `S::Error`. `HtmlError` and
> `VdomError` carry the same two variants and convert in both
> directions. The retained-mode targets (`iced`, `uikit`) deviate from
> the convention because they build widget trees rather than a stream of
> serialized blocks. `html` and `vdom` are compiled unconditionally;
> `ansi`, `dioxus`, `iced` and `typst` are Cargo-feature gated and
> `uikit` is `target_os = "ios"` gated, so `HtmlAdapter` is the one
> renderer always available. `escape` is `pub(crate)` and has no
> exported surface. The sink constructors are deliberately asymmetric:
> `HtmlSink` is built from an inner sink plus an optional custom
> renderer, while `VdomSink::with_adapter` takes an already-configured
> `VdomAdapter` so a caller can apply the full builder chain to it.

## The vdom render tree

> [spec:mw:def:html.vdom.model]
> A `VNode` is the framework-neutral render tree that `VdomAdapter`
> produces, and it is the single source of truth for output structure.
> It has four variants: `Text`, holding an unescaped string; `Element`,
> holding a `'static` tag name, `Props`, child nodes, and the
> serialization hints `nl_open`, `nl_close` and `void`; `Raw`, holding
> pre-rendered HTML (diagram SVG, math SVG, highlighted code, filtered
> raw HTML, custom-renderer output) with `Props` and an optional wrapper
> tag; and `Fragment`, a transparent grouping that contributes only its
> children. `Props` is an insertion-ordered sequence of
> `(&'static str, AttrVal)` pairs, not a map. `AttrVal` records the
> *context* a value must be encoded for rather than carrying
> already-encoded bytes: `Esc` (needs HTML escaping), `Url` (needs
> percent-encoding), `Lit` (already safe, copy verbatim), `Flag`
> (valueless boolean attribute) and `Empty` (empty-valued attribute).
> Deferring the encoding to the consumer is what lets one tree serve
> both targets: the string serializer applies the per-variant encoding,
> while the serde projection used by the JS frontends emits `tag`,
> `props` and `children` (or `html` for `Raw`), collapses `Flag` and
> `Empty` to `true`, and drops the HTML-only whitespace hints entirely.

## Rendering pipeline

> [spec:mw:req:html.render.serializer-over-vdom]
> `HtmlAdapter` MUST hold a `VdomAdapter` and delegate every
> node-to-structure decision to it, contributing only the
> `VNode`-to-`String` serialization step; the HTML shape of a construct
> is therefore changed in `vdom` and never in `html`. Every
> configuration method (`with_html_filter`, `without_data_image_urls`,
> `with_highlighter`, `with_pikchr`, `with_typst`, `with_graphviz`) MUST
> forward to the inner adapter rather than keep a second copy of the
> setting, so the two adapters cannot be configured into disagreement.
> `render` maps one stream item to one stream item: an `Upsert` keeps
> its `id` and `provisional` flag and replaces only `node`, and a
> `Delete` passes through unchanged. Adapters MUST NOT buffer, merge,
> reorder or drop stream items — correlating upserts and deletes is the
> downstream consumer's job.

> [spec:mw:sem:html.serialize.emission]
> Serializing a `VNode` is a single recursive walk that appends into one
> `String`. A `Text` node is HTML-escaped. A `Fragment` emits its
> children and nothing of its own. An `Element` emits `<`, the tag, its
> attributes, `>`, a newline if `nl_open`, its children, then `</`, the
> tag, `>`, and a newline if `nl_close`; a `void` element instead emits
> the open tag followed by ` />` and no children or closing tag. A `Raw`
> node with no wrapper tag emits its payload verbatim, and with a
> wrapper tag emits that tag's open/close pair around the verbatim
> payload; either form appends a newline if `nl_close`. Attributes are
> written the same way for `Element` and `Raw`. The whitespace hints
> exist so that byte-exact output is reproducible from the tree alone,
> without the serializer needing to know which markdown construct
> produced a node.

> [spec:mw:req:html.escape.attribute-contexts]
> Attributes MUST be emitted in `Props` iteration order, since that
> order is the adapter's byte output. A valued attribute MUST be written
> as a space, the key, `="`, the encoded value, and `"` — always
> double-quoted. The encoding MUST be selected by the `AttrVal` variant:
> `Esc` through the HTML escaper, `Url` through the percent-encoder,
> `Lit` copied with no transformation at all. `Flag` MUST emit the bare
> key with no `=` and no quotes; `Empty` MUST emit `key=""`. Because
> `Lit` bypasses encoding entirely, a `Lit` value MUST be constructed
> only from crate-controlled constants or from numbers formatted by the
> crate — class names, table alignments, list `start` values, backref
> indices, aria labels. Any value derived from document text MUST use
> `Esc` or `Url`.

## Escaping

> [spec:mw:req:html.escape.text]
> The HTML escaper MUST replace `&` with `&amp;`, `<` with `&lt;`, `>`
> with `&gt;` and `"` with `&quot;`, and MUST pass every other character
> through unchanged. In particular `'` is not escaped, which is why
> attribute values MUST be double-quoted rather than single-quoted. The
> same escaper serves both text nodes and `Esc` attribute values, so
> those two contexts share exactly one character set. Its input is
> already-decoded text — the parser resolves character references before
> building nodes — so `&` is escaped unconditionally and no attempt is
> made to detect and preserve an entity that was already spelled out in
> the source.

> [spec:mw:req:html.escape.url]
> A `Url` attribute value MUST be percent-encoded, not HTML-escaped.
> ASCII alphanumerics, the unreserved marks `-`, `.`, `_`, `~`, the URL
> syntax characters `:`, `/`, `?`, `#`, `@`, `!`, `$`, `&`, `'`, `(`,
> `)`, `*`, `+`, `,`, `;`, `=`, and `%` itself MUST be preserved; every
> other byte — including `"`, `<`, `>`, space, backtick, `[`, `]`, and
> all non-ASCII, which is UTF-8 encoded first — MUST be percent-escaped.
> The two exclusions carry the load. Preserving `%` keeps an already
> percent-encoded `%XX` sequence from being double-encoded into `%25XX`,
> which would corrupt working links. Encoding the quote and angle
> characters is what prevents a destination from terminating the
> attribute or opening a tag, which HTML-escaping a URL would also
> achieve but at the cost of breaking the URL's own syntax characters.
> Percent-encoding is an encoding concern only: rejecting dangerous
> schemes is a separate upstream gate that has already replaced an
> unsafe destination with an empty string before this function sees it.

## Custom node rendering

> [spec:mw:def:html.custom.hook]
> `RenderVdomCustom<Ext>` (payload plus children to a `VNode`) and
> `RenderHtmlCustom<Ext>` (payload plus children appended to a `String`)
> are the extension hooks for `Node::Custom`, the one node variant the
> adapters cannot render from the core model. Both traits are
> implemented for `()`, which is the default type parameter of
> `HtmlAdapter` and `VdomAdapter` and which rejects every custom node
> with the `UnsupportedCustomNode` error; `with_custom_renderer`
> substitutes a real renderer and changes the adapter's type parameter.
> Both hooks take `&self`, so a custom renderer observes but cannot
> mutate adapter state during a render, and the adapters stay usable
> behind a shared reference.

> [spec:mw:sem:html.custom.bridge]
> `HtmlAdapter` accepts a string-producing `RenderHtmlCustom` by
> wrapping it in a private bridge that implements `RenderVdomCustom`,
> so the string hook composes with the vdom pipeline instead of forking
> it. The string the hook writes becomes a `Raw` node with no wrapper
> tag and no props, which means it reaches the output verbatim and is
> never escaped — a custom renderer owns the safety of what it emits,
> exactly as the raw-HTML filter and the diagram sanitizers own theirs.
> The bridge appends a trailing newline when the custom node has at
> least one block-level child and the emitted string does not already
> end in one, so a block-level extension lands the same line breaks as
> the built-in block elements. Errors raised by the string hook convert
> into the vdom error of the corresponding variant and back again, which
> is why the two error types are mutually convertible.
