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

> adv[input.byte-cap]
> DOT source exceeding `markdawn_graphviz::MAX_DOT_BYTES` (256 KB)
> must be rejected before any libgraphviz call. Catches enormous
> labels, oceans of identifiers, unterminated strings/comments.

> adv[input.brace-depth-cap]
> Outer `{`/`}` nesting exceeding `MAX_BRACE_DEPTH` (64) must be
> rejected. Catches `subgraph{subgraph{…}}` stack attacks.

> adv[input.record-depth-cap]
> `{`/`}` nesting inside a quoted string (record-label syntax)
> exceeding `MAX_RECORD_DEPTH` (16) must be rejected. The outer-
> brace scanner skips contents of `"…"`, so attackers smuggle
> record recursion this way; the scrubber's string-state counter
> catches it.

> adv[input.html-depth-cap]
> `<tag>` / `</tag>` nesting inside an HTML label exceeding
> `MAX_HTML_DEPTH` (16) must be rejected. HTML labels have their
> own recursive parser inside libgraphviz.

> adv[input.node-cap]
> Heuristic identifier count (upper bound on node count) above
> `MAX_NODES` (1 000) must be rejected. The counter overcounts by
> design — every identifier occurrence increments — so it rejects
> borderline adversarial inputs conservatively.

> adv[input.edge-cap]
> Heuristic edge count above `MAX_EDGES` (2 000) must be rejected.
> The counter expands shorthand: `A -> {b c d}` counts as 3 edges;
> `{A B C} -> {X Y}` counts as 6; chained `A -> B -> C` counts as
> 2.

> adv[input.attribute-cap]
> Total `name=value` attribute declarations above `MAX_ATTRIBUTES`
> (2 000) must be rejected. This is a count of declarations, not
> of `[…]` lists — a single list with thousands of attributes must
> also be rejected.

> adv[input.subgraph-cap]
> Occurrences of the `subgraph` keyword above `MAX_SUBGRAPHS`
> (100) must be rejected.

## Attribute neutralisation (pre-render scrubber)

> adv[neutralise.external-file-attrs]
> The attributes `image`, `shapefile`, `fontpath`, `imagepath`,
> `stylesheet`, and absolute-path `fontname` must be renamed
> (libgraphviz drops unknown attrs silently) so libgraphviz does
> not open external files at render time. Case-insensitive.
> Whitespace around `=` tolerated. Applies inside attribute lists
> and at `graph [...]` / `node [...]` / `edge [...]` defaults.

> adv[neutralise.layout-override]
> The `layout=` graph attribute must be renamed. Our
> `gvLayout(gvc, g, "dot")` call picks the layout engine; allowing
> the attribute to override it exposes us to per-engine DoS
> characteristics (neato, fdp, sfdp, twopi all have different
> pathological inputs).

> adv[neutralise.minlen-clamp]
> The `minlen=N` edge attribute must be clamped to at most
> `MAX_MINLEN` (20). Large values cause libgraphviz to insert
> virtual nodes between ranks; `minlen=1000` on a cross-rank edge
> creates 1 000 virtual nodes from small input.

> adv[neutralise.html-img-tag]
> `<IMG …>` tags inside HTML labels must be renamed so
> libgraphviz's HTML-label parser does not open the `SRC` file for
> image sizing.

## Output hardening (post-render SVG sanitiser)

> adv[output.size-cap]
> Rendered SVG byte length exceeding
> `markdawn::svg_sanitize::MAX_SVG_BYTES` (5 MB) must not be
> passed to the host document. Catches the input-to-output
> amplification class (small DOT → huge SVG).

> adv[output.url-allowlist]
> Every `href` / `xlink:href` attribute value in emitted SVG must
> pass `markdawn::url_safety::is_safe_url`. Values failing the
> check must be replaced with `"#"`. Applies whether the URL came
> from graphviz, pikchr, or any other SVG source.

> adv[output.no-script]
> `<script>…</script>` blocks must be removed from emitted SVG.

> adv[output.no-event-handlers]
> `on*="…"` event-handler attributes (e.g. `onclick`, `onload`,
> `onmouseover`) must be removed from emitted SVG.

## Markdown input cap

> adv[markdown.input.byte-cap]
> The markdown parser must support a caller-supplied input-size cap
> via `ParserBuilder::with_max_input_bytes(n)`. When set, cumulative
> bytes across `push_str`, `finish`, and `parse_full_at` exceeding
> the cap must return `Error::InputTooLarge { seen, cap }`. Default
> is unlimited (`None`) so existing callers aren't affected. Useful
> for host apps rendering untrusted markdown that want a hard upper
> bound before the parser starts allocating.

## Markdown URL hardening

> adv[markdown.url-allowlist]
> Markdown link and image destinations (`[text](URL)`,
> `![alt](URL)`, autolinks) must pass `url_safety::is_safe_url`.
> Failing URLs result in empty `href=""` / `src=""` (matches
> cmark-gfm behaviour — link remains visible but inert).

## Runtime isolation (render stage)

> adv[render.timeout]
> Each render call must enforce a wall-clock timeout
> (`RENDER_TIMEOUT_SECS`, 10 s). On timeout, return
> `GraphvizError::TimedOut`. Because libgraphviz is C and cannot
> be safely killed, the stuck worker thread continues running in
> the background until it naturally returns; subsequent callers
> time out on acquiring the global `RENDER_LOCK` rather than
> waiting indefinitely.

> adv[render.panic-capture]
> The render thread must catch Rust panics and return
> `GraphvizError::Panicked(message)` rather than propagating to
> the caller. This catches Rust-level panics only — C-level
> `abort()` / `SIGSEGV` still terminate the host process and are
> residual risk.

> adv[render.serialise]
> Concurrent render calls must serialise via a process-global
> `Mutex`. libgraphviz keeps per-context state that is not thread-
> safe; overlapping calls would race even with our wrapper.

> adv[render.error-callback-capture]
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

> adv[pikchr.input.byte-cap]
> Pikchr source exceeding `markdawn::pikchr_render::MAX_PIKCHR_BYTES`
> (256 KB) must be rejected before calling libpikchr.

> adv[pikchr.render.timeout]
> Each pikchr render call must enforce a wall-clock timeout
> (`PIKCHR_RENDER_TIMEOUT_SECS`, 10 s). On timeout, return
> `PikchrError::TimedOut`. The worker thread cannot be killed from
> outside; it continues running until libpikchr returns.

> adv[pikchr.render.panic-capture]
> The pikchr render thread must catch Rust panics via
> `std::panic::catch_unwind` and return
> `PikchrError::Panicked(message)` rather than propagating to the
> caller.

> adv[pikchr.render.serialise]
> Concurrent pikchr render calls must serialise via a process-global
> `Mutex`. Defensive — libpikchr is reentrant, but the Mutex cost is
> negligible and matches the graphviz wrapper's shape.

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
