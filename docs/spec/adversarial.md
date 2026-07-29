# adversarial — graphviz DoS & injection hardening spec

This document defines the threat model and defensive tactics for
markdawn's graphviz integration. Each requirement is a blockquote
starting with an `adv[…]` marker so Tracey can extract it; source
files reference them via `adv[impl rule.id]` /
`adv[verify rule.id]` annotations.

## Threat model

Adversary: author of untrusted markdown content rendered by a host
application using markdawn. The adversary controls:

- The body of any ```` ```dot ```` / ```` ```graphviz ```` fenced
  code block.
- The destination of any markdown link / image / autolink.
- The content of any inline HTML that survives `HtmlFilter`.

Their goals may be:

- **Cross-site scripting**: get JavaScript into the rendered page.
- **Denial of service**: consume CPU / memory / wall-clock time.
- **Information disclosure**: read files from the host filesystem
  or probe internal network.
- **Process crash**: trigger libgraphviz `abort()` / `SIGSEGV` /
  stack overflow.

Mitigations are in-process by design; subprocess / WASM isolation is
out of scope by explicit decision. libgraphviz C-level crashes
therefore sit in the residual-risk bucket.

## Input caps (pre-render)

> [spec:adv:req:input.byte-cap]
> DOT source exceeding `markdawn_graphviz::MAX_DOT_BYTES` (256 KB)
> must be rejected before any libgraphviz call. Catches enormous
> labels, oceans of identifiers, unterminated strings/comments.

> [spec:adv:req:input.brace-depth-cap]
> Outer `{`/`}` nesting exceeding `MAX_BRACE_DEPTH` (64) must be
> rejected. Catches `subgraph{subgraph{…}}` stack attacks.

> [spec:adv:req:input.record-depth-cap]
> `{`/`}` nesting inside a quoted string (record-label syntax)
> exceeding `MAX_RECORD_DEPTH` (16) must be rejected. The outer-
> brace scanner skips contents of `"…"`, so attackers smuggle
> record recursion this way; the scrubber's string-state counter
> catches it.

> [spec:adv:req:input.html-depth-cap]
> `<tag>` / `</tag>` nesting inside an HTML label exceeding
> `MAX_HTML_DEPTH` (16) must be rejected. HTML labels have their
> own recursive parser inside libgraphviz.

> [spec:adv:req:input.node-cap]
> Heuristic identifier count (upper bound on node count) above
> `MAX_NODES` (1 000) must be rejected. The counter overcounts by
> design — every identifier occurrence increments — so it rejects
> borderline adversarial inputs conservatively.

> [spec:adv:req:input.edge-cap]
> Heuristic edge count above `MAX_EDGES` (2 000) must be rejected.
> The counter expands shorthand: `A -> {b c d}` counts as 3 edges;
> `{A B C} -> {X Y}` counts as 6; chained `A -> B -> C` counts as
> 2.

> [spec:adv:req:input.attribute-cap]
> Total `name=value` attribute declarations above `MAX_ATTRIBUTES`
> (2 000) must be rejected. This is a count of declarations, not
> of `[…]` lists — a single list with thousands of attributes must
> also be rejected.

> [spec:adv:req:input.subgraph-cap]
> Occurrences of the `subgraph` keyword above `MAX_SUBGRAPHS`
> (100) must be rejected.

## Attribute neutralisation (pre-render scrubber)

> [spec:adv:req:neutralise.external-file-attrs]
> The attributes `image`, `shapefile`, `fontpath`, `imagepath`,
> `stylesheet`, and absolute-path `fontname` must be renamed
> (libgraphviz drops unknown attrs silently) so libgraphviz does
> not open external files at render time. Case-insensitive.
> Whitespace around `=` tolerated. Applies inside attribute lists
> and at `graph [...]` / `node [...]` / `edge [...]` defaults.

> [spec:adv:req:neutralise.layout-override]
> The `layout=` graph attribute must be renamed. Our
> `gvLayout(gvc, g, "dot")` call picks the layout engine; allowing
> the attribute to override it exposes us to per-engine DoS
> characteristics (neato, fdp, sfdp, twopi all have different
> pathological inputs).

> [spec:adv:req:neutralise.minlen-clamp]
> The `minlen=N` edge attribute must be clamped to at most
> `MAX_MINLEN` (20). Large values cause libgraphviz to insert
> virtual nodes between ranks; `minlen=1000` on a cross-rank edge
> creates 1 000 virtual nodes from small input.

> [spec:adv:req:neutralise.html-img-tag]
> `<IMG …>` tags inside HTML labels must be renamed so
> libgraphviz's HTML-label parser does not open the `SRC` file for
> image sizing.

> [spec:adv:req:neutralise.numeric-clamps]
> Render-cost-multiplying numeric attributes must be clamped to
> sane maxima: `peripheries` to `MAX_PERIPHERIES` (5),
> `samplepoints` to `MAX_SAMPLEPOINTS` (200), `dpi` to `MAX_DPI`
> (300). The `layers` and `size` attributes have no legitimate use
> in untrusted markdown rendering and are renamed entirely.
> `minlen` is clamped separately by `neutralise.minlen-clamp`.

> [spec:adv:req:neutralise.numeric-overflow]
> Attribute values that look like pathological numeric literals
> (exponent magnitude > `MAX_NUMERIC_EXPONENT_DIGITS` digits, or
> `nan` / `inf` / `infinity` literals) must be replaced with `"0"`.
> Prevents `width=1e9999` style inputs from reaching libgraphviz's
> `strtod`, which has had historical O(n²) parsing issues on
> pathologically long scientific notation.

> [spec:adv:req:neutralise.escape-expansion]
> Each quoted string containing more than
> `MAX_ESCAPE_EXPANSIONS_PER_STRING` (20) render-time escape
> sequences (`\N`, `\G`, `\L`, `\E`, `\T`, `\H`) must be rejected
> with `LabelEscapeFlood`. These escapes get substituted at render
> time with node / graph / label / edge / tail / head names, so a
> default attribute containing many of them applied to many nodes
> via `node [label="…"]` is a billion-laughs amplifier.

## Output hardening (post-render SVG sanitiser)

> [spec:adv:req:output.size-cap]
> Rendered SVG byte length exceeding
> `markdawn::svg_sanitize::MAX_SVG_BYTES` (5 MB) must not be
> passed to the host document. Catches the input-to-output
> amplification class (small DOT → huge SVG).

> [spec:adv:req:output.url-allowlist]
> Every `href` / `xlink:href` attribute value in emitted SVG must
> pass `markdawn::url_safety::is_safe_url`. Values failing the
> check must be replaced with `"#"`. Applies whether the URL came
> from graphviz, pikchr, or any other SVG source.

> [spec:adv:req:output.no-script]
> `<script>…</script>` blocks must be removed from emitted SVG.

> [spec:adv:req:output.no-event-handlers]
> `on*="…"` event-handler attributes (e.g. `onclick`, `onload`,
> `onmouseover`) must be removed from emitted SVG.

> [spec:adv:req:output.text-as-paths]
> Every `<text>` element in emitted SVG must be converted to
> outlined glyph `<path>` elements (using the bundled Liberation
> family — Serif / Sans / Mono, Regular weight, SIL OFL 1.1) with
> an invisible `<text fill="transparent">` overlay preserved for
> text selection, copy/paste, and screen readers.
>
> Face selection follows the `font-family` attribute
> case-insensitively: the first comma-separated token that maps
> to a generic family wins (`sans-serif` / `helvetica` / `arial`
> etc. → Liberation Sans; `monospace` / `courier` / `consolas`
> etc. → Liberation Mono; otherwise Liberation Serif). Liberation
> is metric-compatible with Times / Helvetica / Courier (which is
> exactly what graphviz and pikchr name in their output) so
> advances line up with the engine's own layout calculations and
> `text-anchor="middle"` labels stay centred inside their shapes.
>
> Per-character fallback: if the primary face doesn't have a
> glyph for a codepoint, the converter tries the other two faces
> before giving up. Only when NO Liberation face covers every
> codepoint in the run is the original `<text>` preserved so the
> browser's fallback font can render it (CJK, Deseret, emoji,
> etc.).

> [spec:adv:sem:output.inline-trim]
> Since every SVG we emit is embedded inline in HTML, the XML
> prologue (`<?xml version=…?>`), DOCTYPE (`<!DOCTYPE svg …>`),
> and all `<!-- … -->` comments are dead weight — browsers treat
> them as bogus comments in HTML context and they add ~400 bytes
> per render of graphviz-generated boilerplate. The sanitiser
> strips all three before returning. Also reduces surface for
> comment-injection tricks (attacker-controlled labels that
> manage to contain `-->` sequences).

> [spec:adv:req:output.no-external-entities]
> `<!ENTITY …>` declarations must be removed from emitted SVG.
> Graphviz doesn't emit these itself, but belt-and-suspenders: any
> entity declaration that reaches our output enables XML-bomb
> expansion or external-entity exfiltration in downstream consumers
> that honour them.

> [spec:adv:req:output.no-stylesheet-pi]
> `<?xml-stylesheet …?>` processing instructions must be removed
> from emitted SVG. Attacker-controlled `href` inside the PI would
> reference an external stylesheet; even with our URL allow-list on
> attribute values, the PI as a whole is safer to drop. Handled by
> html5ever's HTML parse context in the sanitiser (PIs other than
> `<?xml version=?>` are parsed as bogus comments and dropped).

> [spec:adv:req:output.animate-href-safe]
> SVG `<animate attributeName="href" values="javascript:…"/>` is a
> known XSS amplifier: the element itself looks benign, and the
> payload sits inside an `attributeName` string that a naive
> `href`-checker misses. `<animate>` and `<set>` must not appear in
> emitted SVG (they're not on the W3C safe-default allow-list and
> are dropped on sanitise).

> [spec:adv:req:output.foreign-object-safe]
> SVG `<foreignObject>` embeds arbitrary HTML inside SVG. It stays
> on the allow-list because MathML / tooltip content legitimately
> uses it, but its contents must be sanitised as HTML — any
> `<script>` or event-handler attribute hosted inside a
> foreignObject must still be dropped.

## Markdown input cap

> [spec:adv:req:markdown.input.byte-cap]
> The markdown parser must support a caller-supplied input-size cap
> via `ParserBuilder::with_max_input_bytes(n)`. When set, cumulative
> bytes across `push_str`, `finish`, and `parse_full_at` exceeding
> the cap must return `Error::InputTooLarge { seen, cap }`. Default
> is unlimited (`None`) so existing callers aren't affected. Useful
> for host apps rendering untrusted markdown that want a hard upper
> bound before the parser starts allocating.

## Markdown URL hardening

> [spec:adv:req:markdown.url-allowlist]
> Markdown link and image destinations (`[text](URL)`,
> `![alt](URL)`, autolinks) must pass `url_safety::is_safe_url`.
> Failing URLs result in empty `href=""` / `src=""` (matches
> cmark-gfm behaviour — link remains visible but inert).

## Runtime isolation (render stage)

> [spec:adv:req:render.timeout]
> Each render call must enforce a wall-clock timeout
> (`RENDER_TIMEOUT_SECS`, 10 s). On timeout, return
> `GraphvizError::TimedOut`. Because libgraphviz is C and cannot
> be safely killed, the stuck worker thread continues running in
> the background until it naturally returns; subsequent callers
> time out on acquiring the global `RENDER_LOCK` rather than
> waiting indefinitely.

> [spec:adv:req:render.panic-capture]
> The render thread must catch Rust panics and return
> `GraphvizError::Panicked(message)` rather than propagating to
> the caller. This catches Rust-level panics only — C-level
> `abort()` / `SIGSEGV` still terminate the host process and are
> residual risk.

> [spec:adv:req:render.serialise]
> Concurrent render calls must serialise via a process-global
> `Mutex`. libgraphviz keeps per-context state that is not thread-
> safe; overlapping calls would race even with our wrapper.

> [spec:adv:sem:render.process-isolation]
> Each render runs in a fork()ed child process. Parent reads the
> SVG via a pipe with a `poll()`-based timeout; on timeout or
> oversized response the child is SIGKILLed and the kernel
> reclaims its memory. If the child crashes (libgraphviz abort /
> SIGSEGV / stack overflow / OOM-killed) the parent observes the
> death via `waitpid` and returns `GraphvizError::ForkCrashed`
> without going down with it.
>
> Works on both Linux and macOS because markdawn-graphviz vendors
> libgraphviz as a submodule and builds a minimal static set
> (cgraph, cdt, gvc, common, dotgen, pack, label, pathplan, xdot,
> util, sparse, vpsc, rbtree, ast, expr, sfio) plus just the `dot`
> layout plugin and `core` SVG renderer — with
> `WITH_QUARTZ=OFF` / no Cocoa, `WITH_PANGOCAIRO=OFF`,
> `with_ortho=OFF`, `with_sfdp=OFF`, `WITH_GD=OFF`, `WITH_RSVG=OFF`,
> `WITH_EXPAT=OFF`, `WITH_WEBP=OFF`, `WITH_X=OFF`, and all language
> bindings (`ENABLE_SWIG=OFF` etc) off. No dlopen / dyld plugin
> loader, no Objective-C runtime, so fork() has no fork-safety
> traps to hit.
>
> System-graphviz mode (`MARKDAWN_GV_USE_SYSTEM=1` or explicit
> `GRAPHVIZ_LIB_DIR`) bypasses the vendored build; on macOS
> Homebrew graphviz pulls Cocoa via its Quartz plugin and fork
> will fail there. The `markdawn_gv_prewarm` callback stays
> installed as a best-effort mitigation for that case.

> [spec:adv:req:render.staged-timeout]
> libgraphviz's pipeline is `agmemread` (parse) → `gvLayout` (layout)
> → `gvRenderData` (render). Each stage must have its own wall-clock
> budget (`PARSE_BUDGET_MS`, `LAYOUT_BUDGET_MS`, `RENDER_BUDGET_MS`)
> enforced inside the C shim *between* stages. We can't interrupt
> libgraphviz mid-call (non-cooperative C), but a stage that exceeds
> its budget must bail before the next stage starts, so a
> pathological parse doesn't consume the layout + render budgets
> too. The C shim returns distinct error codes
> (`MARKDAWN_GV_TIMEOUT_PARSE` / `_LAYOUT` / `_RENDER`) which map to
> `GraphvizError::TimedOutAtParse` / `AtLayout` / `AtRender` with
> the observed elapsed time as payload.

> [spec:adv:req:render.error-callback-capture]
> `agseterrf` must be installed around each render call so
> libgraphviz diagnostics land in `GraphvizError::RenderFailed`
> instead of leaking to the host process's stderr. The previous
> callback must be restored on exit so a single render doesn't
> leave `err_sink` as the process-wide handler.

## Pikchr runtime isolation

The pikchr integration mirrors graphviz's defensive layer. `pikru`
is a pure-Rust wrapper around the embedded libpikchr C library;
libpikchr is reportedly reentrant, but we apply the same input cap /
timeout / panic capture / serialisation as the graphviz path for
uniform posture and defence in depth.

> [spec:adv:req:pikchr.input.byte-cap]
> Pikchr source exceeding `markdawn::pikchr_render::MAX_PIKCHR_BYTES`
> (256 KB) must be rejected before calling libpikchr.

> [spec:adv:req:pikchr.render.timeout]
> Each pikchr render call must enforce a wall-clock timeout
> (`PIKCHR_RENDER_TIMEOUT_SECS`, 10 s). On timeout, return
> `PikchrError::TimedOut`. The worker thread cannot be killed from
> outside; it continues running until libpikchr returns.

> [spec:adv:req:pikchr.render.panic-capture]
> The pikchr render thread must catch Rust panics via
> `std::panic::catch_unwind` and return
> `PikchrError::Panicked(message)` rather than propagating to the
> caller.

> [spec:adv:req:pikchr.render.serialise]
> Concurrent pikchr render calls must serialise via a process-global
> `Mutex`. Defensive — libpikchr is reentrant, but the Mutex cost is
> negligible and matches the graphviz wrapper's shape.

## Typst runtime isolation

The typst integration mirrors the same defensive layer. typst is
pure Rust and each compile builds its own `RenderWorld`, but the
`comemo` memoisation cache is process-wide, so the input cap /
timeout / panic capture / serialisation posture is applied
uniformly. Two entry points exist — SVG (diagram fences) and PDF
(whole documents) — with separate caps and budgets.

> [spec:adv:req:typst.input.byte-cap]
> Typst source exceeding `markdawn::typst::MAX_TYPST_BYTES` (256 KB)
> must be rejected before compiling, and PDF source exceeding
> `MAX_TYPST_PDF_BYTES` (8 MB) before compiling a document. Both
> return `TypstRenderError::InputTooLarge`, checked before the
> timeout budget starts.

> [spec:adv:req:typst.render.timeout]
> Each typst render call must enforce a wall-clock timeout —
> `TYPST_RENDER_TIMEOUT_SECS` (10 s) for SVG,
> `TYPST_PDF_RENDER_TIMEOUT_SECS` (60 s) for PDF, which compiles a
> whole document rather than one fence. On expiry, return
> `TypstRenderError::TimedOut`. The worker thread cannot be killed
> from outside; it continues running until typst returns. On wasm
> there is no usable thread, so the compile runs inline and the
> timeout is not enforced — the byte cap remains the defence.

> [spec:adv:req:typst.render.panic-capture]
> The typst render thread must catch Rust panics via
> `std::panic::catch_unwind` and return
> `TypstRenderError::Panicked(message)` rather than propagating to
> the caller. A worker that dies without sending — a disconnected
> result channel — must also report `Panicked`, so a lost thread
> never surfaces as a hang or a silent success.

> [spec:adv:req:typst.render.serialise]
> Concurrent typst render calls must serialise via a process-global
> `Mutex` (`RENDER_LOCK`), held across compile and export on both
> the SVG and PDF paths. Lock poisoning must not be fatal: the
> guard is recovered with `into_inner()` so one panicking render
> does not permanently disable the renderer.

## Residual risk

Items explicitly NOT mitigated at this layer (documented for
downstream awareness):

- **libgraphviz C-level crashes** (abort, segfault, stack overflow
  inside the layout engine) kill the host process. Complete fix is
  subprocess / sandbox isolation; declined.
- **Concurrent callers of libgraphviz from other parts of the host
  process** (i.e. not through `markdawn-graphviz`) race with us on
  libgraphviz globals. Out of our control.
- **Inter-call plugin state accumulation** inside libgraphviz —
  small, bounded, but not zero.
- **Rate-limiting / queue backpressure** — application-level, not
  a library concern.
- **Memory limits** — no `cgroups`-style hard cap possible in-
  process. Relies on the input-size + structural caps bounding
  allocations loosely.
