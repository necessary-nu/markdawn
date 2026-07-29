# Tracey-flavoured markdown rendering

The `tracey` cargo feature adds a markdown dialect in which requirement
definitions are written as specially-marked blockquotes, plus a synchronous
render pipeline that extracts them alongside frontmatter, headings and inline
code spans. The dialect is an *input format markdawn consumes*; it is unrelated
to any annotation scheme applied to markdawn's own sources.

## Requirement markers

> [spec:mw:syn:traceyext.marker.grammar]
> A requirement marker has the shape `:[<sigil?><prefix>:<id>]`. It is
> recognised only at the very start of a blockquote's first line, after any
> leading spaces or tabs, the `>` character, and one optional space. `<sigil?>`
> is an optional `!`; `<prefix>` is a non-empty run of `[a-z0-9]` (commonly
> `r`, but multi-character prefixes such as `h2` are legal) terminated by a
> `:`; `<id>` is the text up to the first `]`, which must be non-empty and must
> parse as a rule id. The marker token runs from the leading `:` through the
> closing `]` inclusive — the leading `:` is part of the marker, not part of
> the surrounding prose.

> [spec:mw:req:traceyext.marker.non-markers]
> The leading `:` outside the brackets is mandatory and is the sole
> discriminator between a marker and superficially similar markdown. A parser
> MUST NOT treat any of the following as a marker: the bracket-only form
> `[r:id]`; the paragraph form `r[id]` or `r![id]`; GFM admonition keywords
> such as `[!NOTE]` or `[!WARNING]`; or an in-bracket sigil other than `!`
> (`:[>r:id]`, `:[?r:id]`, `:[*r:id]` — those sigils belong to the
> source-annotation grammar, not the spec-authoring one). A blockquote whose
> first line fails any of these checks MUST be left as an ordinary blockquote.
> An occurrence of marker-shaped text anywhere other than a blockquote's first
> line is likewise not a marker.

> [spec:mw:def:traceyext.marker.kind]
> `ReqKind` records which of the two marker shapes was used.
> `ReqKind::Definition` (the `Default`) comes from `:[<prefix>:<id>]` and
> denotes an ordinary requirement, tracked for implementation and verification
> coverage. `ReqKind::Constraint` comes from `:[!<prefix>:<id>]` and denotes a
> cross-cutting property that applies to its heading subtree and is verified
> rather than implemented. The kind is carried unchanged from the marker
> through `ReqPayload` into `ReqDefinition`.

## Rule identifiers

> [spec:mw:def:traceyext.rule-id.model]
> A `RuleId` is a `base` string paired with a `u32` `version`. A `version` of
> `0` means the marker carried no explicit version, so `0` is not an
> addressable version number but the absence of one. `Display` reproduces the
> authored text: bare `base` when the version is `0`, otherwise `base+version`.
> `RuleId` compares against `&str` in both directions by parsing the string
> side and comparing the resulting `RuleId`s, so `"my.req+2"` equals
> `RuleId { base: "my.req", version: 2 }` and does not equal the unversioned
> id with the same base.

> [spec:mw:req:traceyext.rule-id.version-suffix]
> Rule-id parsing splits at the *last* `+`. The empty string MUST be rejected.
> When a `+` is present, the text before it MUST be non-empty and MUST NOT
> itself contain a `+` (so `a+1+2` is rejected), the text after it MUST be a
> non-empty `u32` literal, and an explicit `+0` MUST be rejected — an
> unversioned id is written without a suffix, never as `+0`. Any id that fails
> these checks yields no `RuleId`, and a marker carrying such an id is not
> extracted at all.

## Source spans

> [spec:mw:req:traceyext.span.absolute-offsets]
> This concern's `SourceSpan` is an `offset` plus a `length`, deliberately
> distinct from the crate's `SourceSpan`, which is a `start..end` pair. The
> offset/length form is what tracey needs to rewrite markers in place, so
> converting between the two is the concern's responsibility rather than its
> callers'. Every span reaching a caller — marker spans, req spans, inline-code
> spans — MUST be an absolute byte offset into the *original* markdown string
> passed to `render`, including any frontmatter that was stripped before
> parsing. Node spans produced against the post-frontmatter body MUST therefore
> be shifted by the byte length of the consumed frontmatter region.

## AST extension

> [spec:mw:def:traceyext.extension.req-node]
> `TraceyExt` is the custom-node payload type for this dialect; its sole
> variant `TraceyExt::Req(ReqPayload)` carries the parsed `id`, `kind`,
> `anchor_id`, marker offset and length, source line, and the literal `prefix`
> consumed from the marker. Extraction walks the parsed tree and replaces each
> marked `Node::BlockQuote` with a `Node::Custom` carrying that payload,
> preserving the blockquote's children and source span. The marker token, plus
> any spaces or tabs immediately following it, is removed from the leading
> `Text` nodes of the first inner paragraph so the rendered body never repeats
> the marker; `Text` nodes emptied by the strip are dropped and the survivors'
> span starts are advanced by the number of bytes removed. The anchor id is
> `r--` followed by the slugified `Display` form of the rule id. The payload's
> `line` is left at `0` during extraction and filled in later from the absolute
> marker offset.

## Render pipeline

> [spec:mw:def:traceyext.render.document]
> `render(markdown, &RenderOptions) -> Result<Document, RenderError>` is the
> entry point, shaped to correspond to the marq 2.x `render(md, opts) ->
> Document` API so callers can move between the two. A `Document` carries the
> rendered `html`, an optional `frontmatter`, the `headings`, the `reqs`, an
> `elements` list, `head_injections`, and `inline_code_spans`. `DocElement` is
> the source-ordered union of `Heading`, `ReqDefinition` and `Paragraph` (the
> latter recording only line and offset) and exists for outline and
> table-of-contents construction. `RenderOptions` is a builder exposing
> `with_source_path`, `with_req_handler` and `with_inline_code_handler`; the
> handlers are stored behind `Arc` so options stay `Clone`. Each
> `ReqDefinition` reports the rule id, kind, anchor id, the span of the marker
> alone and of the marker plus body, a 1-indexed line, the `raw` markdown of
> the body with the marker removed (blockquote `> ` prefixes retained), and the
> body `html` without any handler-supplied wrapper. `RenderError` wraps the
> frontmatter, parse, HTML and handler failure modes.

> [spec:mw:sem:traceyext.render.pipeline]
> Rendering runs as a fixed sequence. Frontmatter is split off first and the
> resulting byte-length difference becomes the base offset applied to every
> span thereafter; a frontmatter block whose fields all hold their default
> values is reported as `None` rather than as a present-but-empty block. The
> body is parsed through an `EditableDocument` rather than the streaming
> parser, because only the full-parse path populates `Node::source` byte
> ranges — the streaming push path leaves them zeroed and would make every
> emitted span meaningless. Markers are then extracted into custom nodes; a
> configured inline-code handler rewrites each `CodeSpan` whose `render`
> returns `Some` into a raw `Node::Html`, leaving the rest as code spans;
> metadata is collected in source order; and each custom req node is finally
> replaced by a `Node::Html` holding the handler's opening output, the
> separately rendered body, and the handler's closing output, with
> `DefaultReqHandler` standing in when no handler was configured. Both the
> inner and outer HTML adapters are built with an unrestricted HTML filter:
> the input is project-owned spec markdown treated as trusted, and the
> handler-injected HTML is deliberately passed through, since sanitizing either
> path strips the attributes and classes the rendered output depends on.
> `head_injections` is always empty.

## Frontmatter

> [spec:mw:syn:traceyext.frontmatter.delimiters]
> Frontmatter is recognised only at byte zero, opened by `+++` for TOML or
> `---` for YAML followed immediately by `LF` or `CRLF`. The block ends at the
> earliest line-anchored occurrence of the same delimiter, matched against all
> four `LF`/`CRLF` combinations of leading and trailing line ending; the raw
> region between the delimiters is whitespace-trimmed before parsing. The
> reported `consumed` length covers the opening delimiter, the raw region and
> the closing delimiter, and satisfies `body == &source[consumed..]`. An
> unterminated block is not frontmatter: the whole input is returned as the
> body, with no raw region, no format, and a `consumed` of zero. TOML bodies
> are parsed by `toml` and YAML bodies by `serde_yaml` into the same
> `Frontmatter` (`title`, `weight`, `description`, `template`), every field
> defaulting when absent; a malformed body is a `FrontmatterError::Parse`
> rather than a silent fallback.

## Headings

> [spec:mw:req:traceyext.heading.slug]
> `slugify` MUST keep every character for which `char::is_alphanumeric` holds
> plus `.`, emitting it ASCII-lowercased, and MUST collapse each maximal run of
> any other characters into a single `-`. Leading runs produce no hyphen at all
> and a single trailing hyphen is removed, so the result never begins or ends
> with `-`. Dots survive verbatim, which is what lets a dotted rule id slugify
> into a stable anchor. Heading ids are then built hierarchically: a stack of
> enclosing headings is popped until only strictly-shallower levels remain, and
> the id is each remaining ancestor's slug joined to the heading's own slug by
> `--`. A heading's recorded title is its inline text with emphasis and code
> spans flattened, breaks rendered as single spaces, and the whole trimmed.

## Handlers

> [spec:mw:def:traceyext.handler.traits]
> `ReqHandler` is the `Send + Sync` extension point for wrapping a rendered
> requirement: `start` produces the HTML emitted before the body and `end` the
> HTML emitted after it, each fallible with a `HandlerError`. `InlineCodeHandler`
> is the `Send + Sync` extension point for inline code, with a single `render`
> returning `Some(html)` to substitute raw HTML for the code span or `None` to
> fall back to the default `<code>` element. `DefaultReqHandler` is the
> supplied `ReqHandler`: an anchored `div.req` containing a self-referencing
> `a.req-link` titled and labelled with the rule id, closed by the matching
> `</div>`. Its markup deliberately reproduces marq's `DefaultReqHandler` shape
> so stylesheets written against marq continue to match.

## Markdown diffing

> [spec:mw:sem:traceyext.diff.markdown]
> `diff_markdown` re-parses both inputs into block sequences, serializes each
> block back to markdown, and runs a longest-common-subsequence diff over those
> strings. Unchanged blocks are emitted verbatim, removed blocks wrapped in
> `~~`, added blocks wrapped in `**`, and the result is joined by blank lines;
> an empty block wraps to the empty string rather than to bare delimiters. An
> input that fails to parse contributes no blocks. `diff_markdown_inline`
> follows the same block diff but buffers pending removals: when an addition
> arrives whose block variant — paragraph, heading, blockquote, code block,
> list, or other — matches a buffered removal, the two are merged into a single
> block containing a whitespace-token diff, with runs of removed words wrapped
> in `~~` and runs of added words in `**`. Additions with no matching buffered
> removal flush the buffer first and are emitted wholesale, and any removals
> still buffered at the end are flushed in order.
