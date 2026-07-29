# Dioxus adapter

The Dioxus adapter (`crate::adapter::dioxus`) turns parsed markdown into a
`dioxus::Element` for SSG / SSR composition. It is an *independent*
implementation of the node→structure mapping: unlike `adapter::html`, which
is a thin serializer over the canonical `adapter::vdom` `VNode` tree, this
adapter matches on `Node` directly and builds `rsx!` output. These rules
describe that adapter contract. Which HTML a given markdown construct is
supposed to produce is not restated here — that lives in the `gfm` rules.

## Public surface

> [spec:mw:def:dioxus.surface.entry-points]
> The adapter exposes a two-layer surface. `Renderer<R>` is the configurable
> layer: it owns the `DioxusOptions`, an `HtmlFilter`, an optional
> `AlertIconFn`, an optional `SyntaxHighlighter` (under the `highlight`
> feature) and an ordered list of `CodeBlockRewriter`s, and is built with the
> `with_*` chain. `Renderer::render(&self, source) -> Element` parses and
> renders a whole document; `Renderer::render_node(&self, node) -> Element`
> renders one already-parsed `Node`, block or inline. The convenience layer
> is `render_markdown(source, options)`, defined as
> `Renderer::default().with_options(options).render(source)`; the `Markdown`
> component, which delegates to `render_markdown`; and the free
> `render_node(node, custom)`, which builds a throwaway `Renderer` borrowing
> `custom` and therefore renders with default options and the safe HTML
> filter. `RenderDioxusCustom<Ext>` is the plug-in trait for `Node::Custom`
> payloads, implemented for `()` as a no-op. Because a Dioxus component
> renders behind a shared reference, `render` and `render_node` take `&self`
> while `SyntaxHighlighter::highlight` takes `&mut self`; the highlighter
> slot is consequently a `RefCell`, which — together with the boxed
> `AlertIconFn` — makes `Renderer` neither `Send` nor `Sync`.

> [spec:mw:def:dioxus.surface.options+1]
> `DioxusOptions` carries one `bool` per parser feature (`gfm`, `math`,
> `alerts`, `highlights`, `definition_lists`), each mapping one-to-one onto
> the corresponding `Parser::builder()` call — `Renderer::render` rebuilds a
> `Parser` from them on every call — plus one render-time toggle per
> compiled-in diagram engine (`pikchr`, `graphviz`, `typst`), each present
> only under its cargo feature and each gating the code-block dispatch for
> its own info word. Every field MUST be read by the renderer: a flag with
> no dispatch behind it is a public knob that does nothing.
> `DioxusOptions::all()` turns on everything the current feature set ships,
> so a field added under a cargo feature MUST be added to `all()` in the
> same breath. Renderer-side configuration — HTML filter, highlighter, alert
> icons, code-block rewriters, custom-extension renderer — is deliberately
> *not* part of `DioxusOptions`; it is reachable only through `Renderer`.

## Component contract

> [spec:mw:req:dioxus.props.inert]
> The `Markdown` component's props MUST remain plain owned values that
> satisfy Dioxus's `Clone + PartialEq` prop bounds — today `source: String`
> and `#[props(default)] options: DioxusOptions`, the latter deriving
> `Clone + Copy + PartialEq + Eq + Default` for exactly this reason. A prop
> holding a closure, a trait object or interior mutability would defeat
> Dioxus's prop-equality memoization, so the extension points MUST NOT be
> promoted to props: callers who need a highlighter, alert icons, a
> non-default HTML filter, code-block rewriters or a custom-extension
> renderer build a `Renderer` once and embed `renderer.render(source)` into
> their own `rsx!` rather than using the component. The emitted tree MUST
> carry no event handlers, and the component MUST call no hooks or signals,
> so that its output is identical under `dioxus_ssr::render_element` and in a
> live `VirtualDom` and re-renders only when `source` or `options` changes.
> The one caller-supplied `Element` that enters the tree is the alert icon:
> `AlertIconFn` is invoked during rendering, once per alert, and its result
> is placed inline in the alert title row — it is never stored in the tree as
> a callback.

## Node → `rsx!` mapping

> [spec:mw:req:dioxus.structure.vdom-agreement+1]
> For every `Node` variant the tag, nesting and `class` / `id` / `data-*`
> values emitted here MUST agree with the `VNode` tree `adapter::vdom` builds
> for the same node, so that a document rendered through `dioxus_ssr` and the
> same document rendered through `HtmlAdapter` are structurally
> interchangeable. Because the two are separate matches over `Node`, a change
> to one renderer's structure for a node kind requires the matching change
> here, and `vdom` is the reference: where the two disagree it is this
> adapter that is wrong unless the divergence is listed below. Whitespace is
> out of scope: `VNode` carries `nl_open` / `nl_close` serialization hints
> with no `rsx!` counterpart, so the two outputs are never byte-identical.
> In particular the structures that MUST match are `<dl>` around
> `<dt>` / `<dd>`; `<thead>` unconditionally with `<tbody>` only when the
> table has body rows; a cell's alignment resolved as its own `alignment`
> falling back to the table-level `alignments` entry for its column index;
> `<img src>` screened with `is_safe_image_url`, so `data:image/*` survives
> on both sides; math that fails to typeset falling back to
> `<pre><code class="language-math math-display math-error">` /
> `<code class="language-math math-inline math-error">`; and the code-block
> dispatch recognising the same info words, `ansi` and `terminal`
> short-circuiting ahead of every other branch — including the highlighter —
> whether or not their cargo features are on, and `typst` joining `pikchr`
> and `graphviz` as a feature-and-option-gated diagram engine. The sanctioned
> divergences are:
>
> - table cells carry `style: "text-align: …"` instead of `align="…"`, set
>   unconditionally, to the empty string when the cell resolves to no
>   alignment. A `Node::TableCell` handed straight to `render_node` still
>   renders its `<td>` / `<th>`, where vdom yields an empty fragment.
> - attributes vdom omits when absent are set unconditionally here: `<a>` and
>   `<img>` always get a `title`, and `<ol>` always gets a `start` defaulted
>   to `1`.
> - valueless flag attributes are written with an explicit value —
>   `"data-footnotes": "true"`, `"data-footnote-ref": "true"`,
>   `"data-footnote-backref": "true"` — where vdom emits a bare attribute.
> - the alert title paragraph wraps its label in
>   `<span class="markdown-alert-label">`, preceded by
>   `<span class="markdown-alert-icon">` when an `AlertIconFn` is installed;
>   vdom emits the bare label text.
> - every pre-rendered-HTML payload gains the host element
>   `[spec:mw:req:dioxus.html.host-element]` requires, where vdom splices it
>   bare: diagram SVG lands inside `<div class="pikchr">` /
>   `<div class="graphviz">` / `<div class="typst">` rather than being
>   emitted verbatim under vdom's `aria-label`, and an inline `Node::Html`
>   gains a block-level `<div>`.
> - a `Node::MathBlock` reached in *inline* position keeps its block
>   spelling here — `<div>`, or `<pre><code>` on typeset failure — where vdom
>   switches to `<span>` / bare `<code>`; the classes agree either way.
>
> Attribute *spelling* follows `rsx!` rules, not HTML: Rust keywords are
> raw-escaped (`r#type`, `r#for`), and any HTML attribute name that is not a
> Rust identifier MUST be written as a quoted string key
> (`"aria-label": …`, `"data-footnotes": …`). URL screening is the adapter's
> job — `rsx!` escapes attribute values but does not reject schemes — so
> `href` and `src` are passed through `crate::url_safety` before being handed
> to `rsx!`, and a rejected URL becomes the empty string rather than an
> omitted attribute.

> [spec:mw:req:dioxus.html.host-element+1]
> `rsx!` has no raw-text node: the only way to place pre-rendered markup into
> a Dioxus tree is the `dangerous_inner_html` attribute, which belongs to an
> element and is mutually exclusive with that element's children. Every
> payload that is already HTML therefore MUST be given a host element rather
> than spliced: `Node::Html` into `<div>`, math into
> `<div class="math math-display">` / `<span class="math math-inline">`,
> pikchr into `<div class="pikchr">`, graphviz into
> `<div class="graphviz">`, typst into `<div class="typst">`, highlighted
> code into the `<code>` inside `<pre>`, and each `ansi` or `terminal` pane
> into its own `<code>` — the rendered pane under `language-ansi` /
> `language-terminal`, the annotated-source pane under
> `language-ansi-source` / `language-terminal-source`, both inside the
> two-pane `<div class="ansi-block">` / `<div class="terminal-block">`
> whose `<label for>` addresses its sibling checkbox `<input id>`. Two
> consequences are
> load-bearing. First, an *inline* `Node::Html` gets a block-level `<div>`
> host, so raw HTML that the HTML adapter splices mid-paragraph appears here
> as a nested `<div>` — the one place the inline/block distinction is lost.
> Second, `dangerous_inner_html` performs no filtering of its own, so every
> string reaching it MUST already be sanitized by markdawn: `Node::Html` is
> passed through the `Renderer`'s `HtmlFilter`, which defaults to
> `HtmlFilter::safe()` and is relaxed only by an explicit
> `with_html_filter(HtmlFilter::unrestricted())`; diagram SVG is passed
> through `svg_sanitize` (including the byte cap) and `svg_text_to_paths`
> before it is wrapped.

## Failure behaviour

> [spec:mw:sem:dioxus.render.totality]
> The adapter has no error channel — every entry point returns `Element` and
> every failure degrades to output rather than propagating. A `parse_full_at`
> failure yields an empty element and discards the diagnostic, so a caller
> that needs to know a document was malformed must parse it separately.
> `Node::Custom` under the `()` renderer yields an empty element, where the
> equivalent `RenderHtmlCustom for ()` returns
> `HtmlError::UnsupportedCustomNode`. A diagram backend that errors, or that
> produces an SVG over the sanitizer's byte cap, falls through to the next
> code-block branch and ultimately to plain `<pre><code>`. The adapter also
> exposes no streaming surface: it has no `StreamItem` / `Sink`
> counterpart to `HtmlSink` and no provisional-emission handling, so it
> renders whole documents only.
