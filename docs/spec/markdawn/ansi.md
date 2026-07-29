# ansi — escape handling, terminal emulation, ANSI adapter

Two independent contracts share one colour model and must not be
conflated:

- **Consuming** (ANSI in): `ansi_to_html` turns a byte stream carrying
  SGR + OSC 8 into an HTML fragment; `terminal` runs the same bytes
  through a real VT100 emulator and snapshots the visible grid.
- **Emitting** (ANSI out): `AnsiAdapter` walks a `Node<Ext>` tree and
  writes styled terminal text; `terminal::render_grid_ansi` re-encodes a
  snapshotted grid back to SGR.

`ansi_style` is the shared vocabulary both directions agree on. All
three modules sit behind the `ansi` feature; `terminal` additionally
behind `terminal`.

## Shared colour and cell-style model

> [spec:mw:def:ansi.style.color]
> A `Color` is one of `Named(u8)` (a 0–15 index covering SGR 30–37 /
> 90–97 and their background twins), `Palette(u8)` (an xterm 256-colour
> index) or `Rgb(u8, u8, u8)` (24-bit truecolour). Palette resolution is
> deferred to render time: `to_css` dispatches `Named` to the fixed
> xterm table, `Palette` to the 256-index arithmetic, and `Rgb` to a
> lowercase `#rrggbb` literal. `Named` is kept distinct from
> `Palette(0..=15)` so the SGR re-encoding direction can recover the
> short 30–37 / 90–97 forms.

> [spec:mw:def:ansi.style.cell-style]
> A `CellStyle` is the whole attribute state of one terminal cell:
> `fg` / `bg` as `Option<Color>` plus the independent booleans `bold`,
> `dim`, `italic`, `underline`, `strike`, `inverse`. It is `Copy` and
> its `Default` is the plain style (no colours, no attributes).
> `is_plain()` is defined as equality with that default, and is the
> single predicate both renderers use to decide whether any styling
> wrapper is needed at all.

> [spec:mw:sem:ansi.style.palette-resolution]
> `named_palette(n)` returns the xterm default for `n` in 0–15 (normals
> on 0–7, brights on 8–15) and the CSS keyword `inherit` for anything
> above 15, so an out-of-range index degrades to the ambient colour
> rather than a wrong one. `ansi256_css(n)` splits the 256-index space
> three ways: 0–15 delegate to `named_palette`; 16–231 form a 6×6×6
> cube whose per-channel level is `idx/36 % 6`, `idx/6 % 6`, `idx % 6`
> over `idx = n - 16`, each level `v` mapped to the byte `0` when `v`
> is 0 and `55 + 40v` otherwise; 232–255 form a 24-step grey ramp with
> all three channels set to `8 + 10 * (n - 232)`.

> [spec:mw:req:ansi.style.css-emission]
> `CellStyle::css` MUST emit declarations in a fixed order — `color`,
> `background`, `font-weight`, `opacity`, `font-style`,
> `text-decoration` — joined by `;` with no trailing semicolon, so two
> equal styles always produce byte-identical strings and run-coalescing
> can compare on the style rather than the text. `inverse` MUST be
> resolved by swapping `fg` and `bg` before emission, not by an extra
> declaration, because CSS has no inverse primitive. `underline` and
> `strike` MUST collapse into a single `text-decoration` declaration
> (`underline line-through` when both are set), since emitting two
> `text-decoration` declarations would leave only the last one live.
> Attributes with no CSS analogue emit nothing; `bold` is `700` and
> `dim` is `opacity: 0.6`.

## Consuming — SGR byte stream to HTML

> [spec:mw:def:ansi.to-html.entry]
> `ansi_to_html(source) -> String` drives a `vte::Parser` over the UTF-8
> bytes of `source` and returns an HTML *fragment* — styled `<span>`
> runs and OSC 8 `<a>` wrappers, with no enclosing element and no
> `<pre>`. It interprets exactly two things: SGR (a CSI whose final byte
> is `m`) and OSC 8. It is a code-block converter, not an emulator: it
> holds no grid, no cursor and no scroll region. Callers wanting the
> *rendered* result of cursor traffic must use `terminal` instead.

> [spec:mw:req:ansi.to-html.non-sgr-dropped]
> A CSI dispatch whose final byte is not `m` MUST be discarded
> immediately: no output bytes, no placeholder, and no mutation of the
> running `CellStyle`. Cursor movement, erase-display, scroll-region and
> mode-set sequences therefore vanish without disturbing the styled runs
> around them. The same applies to ESC and DCS dispatches, which have no
> handler at all on this writer.

> [spec:mw:req:ansi.to-html.sgr-params]
> The recognised SGR parameters are exactly: `0` reset-all; `1` bold;
> `2` dim; `3` italic; `4` underline; `7` inverse; `9` strike; `22`
> clearing bold *and* dim together; `23`, `24`, `27`, `29` clearing
> italic, underline, inverse, strike respectively; `30`–`37` foreground
> `Named(code - 30)`; `38` extended foreground; `39` foreground back to
> default (`None`); `40`–`47` background `Named(code - 40)`; `48`
> extended background; `49` background back to default; `90`–`97`
> foreground `Named(code - 90 + 8)`; `100`–`107` background
> `Named(code - 100 + 8)`. Every other parameter — including blink (5),
> conceal (8), double-underline (21), overline (53) and the
> underline-colour selectors (58/59) — MUST be skipped silently, without
> aborting the rest of the sequence and without emitting a marker. There
> is no separate "bright" flag: brightness is folded into the colour
> index, so `SGR 1` after `SGR 31` does not change the hue.

> [spec:mw:sem:ansi.to-html.sgr-accumulation]
> SGR parameters mutate one running `CellStyle` left to right, so within
> a single escape a later parameter overrides an earlier one. `0`
> appearing mid-list resets everything accumulated so far — including
> parameters from the *same* escape — and the parameters after it apply
> to the freshly-defaulted style, which makes `ESC[31;0;1m` bold and
> uncoloured. A parameterless `ESC[m` is treated as `SGR 0`. The style
> survives across printable text, newlines and dropped sequences; only
> an explicit reset, an attribute-clearing code or a colour-default code
> retires part of it, so a run left open at end of input simply ends.

> [spec:mw:req:ansi.to-html.extended-color]
> `38` and `48` MUST consume their selector and operands from the same
> parameter iterator the outer loop is walking. Selector `5` consumes
> one further parameter as a 256-palette index; selector `2` consumes
> three further parameters as R, G, B. Operands are narrowed to `u8` by
> truncation, so values above 255 wrap rather than erroring. If the
> parameter list ends before the operands are complete, the colour is
> left unchanged and the sequence ends quietly. For an unrecognised
> selector only the selector itself has been consumed, so the remaining
> parameters fall back to the outer loop and are re-read as ordinary SGR
> codes. Only the *first* sub-parameter of each parameter is read, so
> the colon-delimited variants (`ESC[38:5:9m`, `ESC[38:2::r:g:bm`) are
> not honoured — they are seen as a single `38` with nothing following.

> [spec:mw:req:ansi.to-html.span-discipline]
> At most one `<span>` may be open at any point, and spans MUST NOT
> nest. A style change closes the open span before opening its
> replacement, so the `</span>` for a run is always emitted before the
> `<span>` of the next one and no interleaving is possible. A plain
> style opens nothing, so no empty `<span style="">` is ever produced
> and unstyled text stays bare. Opening is lazy — a style change alone
> does not emit a tag; the span materialises when the first printable
> character arrives — so a run of escapes with no text between them
> collapses to nothing. `finish` closes the open span before returning.

> [spec:mw:req:ansi.to-html.osc8]
> An OSC dispatch is acted on only when its first parameter is exactly
> `8`; every other OSC is dropped. The URL is the *third* parameter
> (index 2), matching the `ESC ] 8 ; params ; URL ST` shape; an absent
> or empty URL is the closing form and only closes the anchor. Opening
> and closing MUST both close any open `<span>` *before* touching the
> `<a>`, so spans always nest strictly inside anchors and never straddle
> the anchor boundary; `finish` likewise closes span before link. The
> emitted `href` escapes `&`, `<` and `"` — `>` is deliberately not
> escaped, being inert inside a quoted attribute — and the URL is
> otherwise passed through verbatim: no scheme allowlist, percent-
> encoding or normalisation is applied at this layer.

> [spec:mw:req:ansi.to-html.text-and-controls]
> Printable characters MUST be HTML-escaped for `&`, `<`, `>` and `"`
> only; `'` and everything else are emitted literally. Of the C0
> controls only `\n`, `\t` and `\r` survive, emitted as themselves;
> every other executed control byte is dropped. A newline MUST close the
> open span before it is written, so line breaks always sit outside
> styled runs and the fragment stays well-formed when a consumer splits
> it on `\n`. The running style is *not* reset by that close, so the
> next printable character reopens an equivalent span and colour carries
> across lines the way a terminal would show it.

## Consuming — annotated source pane

> [spec:mw:syn:ansi.annotated.labels]
> `ansi_to_annotated_source(source)` returns a fragment in which every
> escape is rendered as *visible text* wrapped in
> `<span class="ansi-esc">…</span>`, so a reader can inspect the
> sequences without a browser or terminal interpreting them. The label
> forms are: CSI → `ESC[` then parameters joined by `;` with
> sub-parameters joined by `:`, then the final byte; ESC → `ESC ` then
> the intermediates then the final byte; OSC → `OSC ` then parameters
> joined by `;` then ` ST`, with any byte that is neither ASCII-graphic
> nor space rendered as `^XX`; a C0 control other than `\n`, `\t`, `\r`
> → `^XX` in its own labelled span. Printable characters pass through
> with the same four-entity escaping as the rendering path, and `\n`,
> `\t`, `\r` are emitted literally so the source keeps its layout.

## Consuming — terminal grid emulation

> [spec:mw:def:ansi.term.grid]
> A `Cell` is one rendered grid position: `ch`, the string the terminal
> would draw there, plus the `CellStyle` it would draw it with. `ch` is
> a `String` rather than a `char` because a cell may hold a grapheme
> cluster, and because a wide character claims two consecutive cells
> with the second holding the empty string. A cell `is_blank` when its
> style is plain and its content is empty or a single space — the
> predicate both renderers use to trim trailing run-out. A `TermGrid` is
> an owned snapshot: `rows` × `cols` of `Cell`, the `(row, col)` cursor,
> an optional `title`, and the `alternate_screen` flag. `title` is
> always `None` on the current `vt100` 0.16, which exposes no OSC 0/2
> title accessor.

> [spec:mw:def:ansi.term.emulator]
> `TermEmulator` wraps a `vt100::Parser` over a fixed `rows` × `cols`
> grid with `scrollback` retained rows; `new()` is 24 × `DEFAULT_COLS`
> (80) with no scrollback. It is stateful and whole-buffer: `feed` /
> `feed_str` may be called any number of times, then `snapshot` takes an
> owned `TermGrid` of the *visible* screen. Because a real emulator is
> underneath, cursor movement, erase operations, scroll regions,
> line wrapping at `cols` and alternate-screen swaps all take effect on
> the grid instead of being dropped — the snapshot is what a human would
> see, not the literal byte stream. There is no streaming equivalent.

> [spec:mw:req:ansi.term.style-mapping]
> Snapshotting MUST carry `fg`, `bg`, `bold`, `italic`, `underline` and
> `inverse` across from the emulator cell. `dim` and `strike` MUST be
> left `false` unconditionally: `vt100::Cell` 0.16 exposes no accessor
> for either, so a dim or struck source run is snapshotted as plain in
> those two respects. Colour maps as `Default` → `None`, `Idx(n)` with
> `n < 16` → `Named(n)`, any other `Idx(n)` → `Palette(n)`, and
> `Rgb(r, g, b)` → `Rgb(r, g, b)`; the `n < 16` split is what lets the
> re-encoding direction recover short SGR forms. A cell with no contents
> yields an empty `ch`, and a position the emulator does not report
> yields the default `Cell`.

> [spec:mw:req:ansi.term.auto-size]
> `ansi_snapshot_grid` MUST clamp `cols` to at least 1, defaulting to
> `DEFAULT_COLS` (80) — the width is load-bearing because the emulator
> wraps on it. When `rows` is supplied the grid is clamped to at most
> `MAX_AUTO_ROWS` (200), at least 1, and returned at exactly that
> height with no trimming, so the caller's geometry is honoured
> verbatim. When `rows` is omitted the content is emulated at
> `MAX_AUTO_ROWS` and then trimmed to
> `max(final cursor row, last non-blank row) + 1` rows, so a transcript
> that painted three lines embeds as three rows rather than 200, while a
> program that parked the cursor below its output keeps that trailing
> space. The cursor row participates in the maximum precisely so the
> trim never cuts above where the next write would land.

## Emitting — grid to HTML and back to ANSI

> [spec:mw:req:ansi.term.html-runs]
> `render_grid_html` MUST trim each row at its last non-blank cell, so
> the 80-column padding of a snapshot does not become 80 columns of
> markup. Adjacent cells with *equal* styles MUST coalesce into one
> `<span>`: a span is closed when the style changes and reopened only if
> the new style is non-plain, so a plain run is emitted bare and a
> styled run is never split cell-by-cell. Cell content is escaped for
> the same four entities as the byte-stream path, and an empty `ch`
> renders as a single space so a wide character's trailing half does not
> shorten the line. Rows are joined by `\n` with no newline after the
> last row, leaving the enclosing `<pre>` to the caller.

> [spec:mw:syn:ansi.term.ansi-encoding]
> `render_grid_ansi` re-encodes a grid as SGR text using
> reset-then-set rather than differential updates: on every style change
> it writes `ESC[0m` first — including a change *into* a plain style —
> and then the new style's SGR only if that style is non-plain, and it
> writes a final `ESC[0m` at end of row when the row ended non-plain.
> Each run is therefore self-contained and no attribute can leak past
> its cell range. Rows are trimmed at the last non-blank cell, an empty
> `ch` becomes a space, and rows are joined by `\n` with none after the
> last. `style_to_sgr` orders parameters bold `1`, dim `2`, italic `3`,
> underline `4`, inverse `7`, strike `9`, then foreground, then
> background, joined by `;` inside one `ESC[…m`, and yields the empty
> string for a plain style. `color_to_sgr` encodes `Named(n)` with
> `n < 8` as `30 + n` / `40 + n`, `Named(n)` with `n >= 8` as
> `90 + (n - 8)` / `100 + (n - 8)`, `Palette(n)` as `38;5;n` / `48;5;n`,
> and `Rgb(r, g, b)` as `38;2;r;g;b` / `48;2;r;g;b`.

## Emitting — node tree to ANSI

> [spec:mw:def:ansi.adapter.surface]
> `AnsiAdapter<R>` renders a `Node<Ext>` tree to styled terminal text.
> It is stateless across renders — all configuration lives on the `Copy`
> `AnsiOptions` (`width`, `hyperlinks`, `color`) installed by
> `with_options` — so one adapter may be reused for any number of
> `render`, `render_node` and `render_many` calls. `render` maps a
> `StreamItem<Node<Ext>>` to a `StreamItem<String>`, passing `Delete`
> through untouched and preserving the emission's `id` and `provisional`
> flag. `Node::Custom` payloads are delegated to the `R: RenderAnsiCustom<Ext>`
> plug-in, which receives an `AnsiWalker` holding block and inline
> recursion as two closures so the custom renderer can recurse without
> holding a `&self` reference to the adapter. The default plug-in `()`
> rejects every payload with `AnsiError::UnsupportedCustomNode`;
> `AnsiError::Sink` carries a stringified downstream error from
> `AnsiSink`, the streaming wrapper symmetrical with `HtmlSink`.

> [spec:mw:sem:ansi.adapter.color-depth-inert]
> `ColorDepth` is `Basic16` / `Ansi256` / `TrueColor` and defaults to
> `Ansi256`. It is carried on `AnsiOptions` and re-exported from the
> crate root, but the adapter never reads it: every SGR sequence the
> adapter emits is a fixed literal, so no
> truecolour value is downgraded to a 256-palette or 16-colour
> approximation and setting `Basic16` does not change a single output
> byte. The field is configuration surface only. Content that carries
> truecolour reaches the terminal unmodified, which is correct for the
> `ansi` code-block passthrough path and a silent no-op everywhere else.

> [spec:mw:req:ansi.adapter.reset-discipline]
> Every styled run the adapter opens MUST be closed explicitly, and the
> closing code is chosen by whether the run may be nested. Inline
> emphasis, strong and strikethrough close with their *specific* disable
> codes — `ESC[23m`, `ESC[22m`, `ESC[29m` — so that an enclosing style
> survives the inner run; using `ESC[0m` there would strip the outer
> attribute. Runs that set a background or combine attributes — code
> spans, highlights, footnote refs, headings, the dim quote and alert
> bars, thematic breaks, definition terms — close with `ESC[0m`, which
> is why they MUST NOT be relied on to preserve an enclosing style. Note
> that `ESC[22m` clears dim as well as bold, so a strong run nested
> inside a dim run ends the dimming too.

> [spec:mw:req:ansi.adapter.code-block]
> A fenced block dispatches on the first whitespace-delimited word of
> its info string, ignoring any further words. `ansi` means the literal
> is already styled and MUST be emitted verbatim, with a single
> `ESC[0m` appended when it does not already end in one so the style
> cannot leak into following blocks. `terminal` means the literal is a
> transcript: under the `terminal` feature it is run through the
> emulator and re-emitted from the final visible screen, discarding the
> cursor traffic entirely; without that feature it falls back to
> verbatim passthrough. Any other info string (or none) is treated as
> source code: a dim language label line when a language is present,
> then each line prefixed with `ESC[48;5;236m` and two spaces and
> terminated with `ESC[0m` before its newline, so the background band
> stops at the text rather than running to the terminal edge. All three
> forms guarantee a trailing newline and then a blank separator line.

> [spec:mw:req:ansi.adapter.links]
> With `hyperlinks` enabled a link MUST be emitted as
> `ESC]8;;<destination>ESC\`, the link text, then the closing
> `ESC]8;;ESC\` — ST-terminated, not BEL-terminated. With `hyperlinks`
> disabled the text is emitted plain and followed by ` (<destination>)`
> so the URL stays readable on a terminal that would otherwise show the
> raw escape. The destination is written verbatim in both forms: it is
> neither escaped nor validated here, so a destination containing `ESC`
> or `ST` would terminate the sequence early.

> [spec:mw:req:ansi.adapter.wrapping]
> Column arithmetic MUST run on *visible* length, never byte or `char`
> length of the styled string. `visible_len` skips a CSI by consuming
> through the first ASCII-alphabetic byte after `ESC[`, and skips an OSC
> by consuming after `ESC]` until BEL or until an `ESC` plus one more
> byte; every other `char` counts as width 1. It does no wcwidth, so
> wide and combining characters are mis-measured — accepted for wrapping
> and table column widths. `write_wrapped` treats width 0 as
> "no wrapping" and emits the input unchanged; otherwise it splits each
> line on whitespace, which collapses runs of spaces and drops leading
> whitespace, and moves a word to the next line when
> `col + 1 + visible_len(word) > width`. Over-long words MUST NOT be
> broken — they overflow the width rather than being split mid-token,
> which would sever an escape sequence. Newlines already present in the
> input are honoured verbatim and reset the column. Because escapes cost
> zero columns and ride along with their word, a wrapped line may carry
> a style opened on a previous line.
