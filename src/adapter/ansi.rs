//! ANSI adapter — walks a `Node<Ext>` tree and emits a string of
//! styled text suitable for terminals (16/256/truecolor SGR + OSC 8
//! hyperlinks).
//!
//! Follows the same shape as `adapter/html.rs` / `adapter/typst.rs`:
//! `AnsiAdapter<R>` + `RenderAnsiCustom<Ext>` plug-in + closure-based
//! `AnsiWalker` for custom renderers that need to recurse into their
//! own children. `AnsiSink` for streaming use.
//!
//! Only code that emits bytes is in here. ANSI *parsing* (e.g. for
//! rendering a `` ```ansi `` code block into HTML) lives in the
//! top-level `ansi_to_html` module.

use core::fmt;

use crate::node::{AlertKind, Emission, Node, StreamItem, TableAlignment};
use crate::sink::Sink;

/// Errors surfaced by the ANSI adapter or its streaming sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnsiError {
    UnsupportedCustomNode,
    Sink(String),
}

impl fmt::Display for AnsiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCustomNode => f.write_str("custom node renderer not configured"),
            Self::Sink(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AnsiError {}

/// Target terminal's color depth. Callers pick based on environment
/// detection; the adapter downgrades truecolor to the nearest 256 or
/// 16 entry as needed so output stays readable on older terminals.
// Configuration surface only: the render path emits fixed SGR literals
// and never reads this field, so no truecolour value is downgraded and
// `Basic16` does not change a single output byte.
// [spec:mw:sem:ansi.adapter.color-depth-inert]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    Basic16,
    Ansi256,
    TrueColor,
}

impl Default for ColorDepth {
    fn default() -> Self {
        ColorDepth::Ansi256
    }
}

/// Render configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnsiOptions {
    /// If `Some(w)`, wrap paragraphs to `w` visible columns. `None`
    /// means emit as-is and let the terminal soft-wrap.
    pub width: Option<usize>,
    /// Emit OSC 8 hyperlinks (supported by iTerm2, WezTerm, VTE, Kitty,
    /// GNOME Terminal 3.26+, Windows Terminal). Falls back to
    /// `text (url)` when disabled.
    pub hyperlinks: bool,
    /// Terminal's colour depth. Defaults to 256-colour.
    pub color: ColorDepth,
}

/// Renderer plug-in for `Node::Custom<Ext>` payloads — the ANSI
/// equivalent of `RenderHtmlCustom<Ext>`.
pub trait RenderAnsiCustom<Ext> {
    fn render_custom(
        &self,
        payload: &Ext,
        children: &[Node<Ext>],
        out: &mut String,
        walker: &AnsiWalker<'_, Ext>,
    ) -> Result<(), AnsiError>;
}

impl<Ext> RenderAnsiCustom<Ext> for () {
    fn render_custom(
        &self,
        _payload: &Ext,
        _children: &[Node<Ext>],
        _out: &mut String,
        _walker: &AnsiWalker<'_, Ext>,
    ) -> Result<(), AnsiError> {
        Err(AnsiError::UnsupportedCustomNode)
    }
}

/// Recursion helper handed to custom renderers. Two closures — one for
/// block-level children, one for inline — let the custom implementer
/// call back into the adapter without holding a `&self` reference to
/// the adapter type (which would make the trait impl self-referential).
pub struct AnsiWalker<'a, Ext> {
    #[allow(clippy::type_complexity)]
    walk_block: &'a dyn Fn(&Node<Ext>, &mut String) -> Result<(), AnsiError>,
    #[allow(clippy::type_complexity)]
    walk_inlines: &'a dyn Fn(&[Node<Ext>], &mut String) -> Result<(), AnsiError>,
}

impl<'a, Ext> AnsiWalker<'a, Ext> {
    pub fn write_block(&self, node: &Node<Ext>, out: &mut String) -> Result<(), AnsiError> {
        (self.walk_block)(node, out)
    }
    pub fn write_inlines(&self, nodes: &[Node<Ext>], out: &mut String) -> Result<(), AnsiError> {
        (self.walk_inlines)(nodes, out)
    }
}

/// The ANSI adapter. Stateless across renders; all configuration lives
/// on `AnsiOptions`.
// [spec:mw:def:ansi.adapter.surface]
pub struct AnsiAdapter<R = ()> {
    custom: R,
    options: AnsiOptions,
}

impl Default for AnsiAdapter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl AnsiAdapter<()> {
    pub fn new() -> Self {
        Self {
            custom: (),
            options: AnsiOptions {
                width: None,
                hyperlinks: true,
                color: ColorDepth::Ansi256,
            },
        }
    }

    pub fn with_custom_renderer<Custom>(custom: Custom) -> AnsiAdapter<Custom> {
        AnsiAdapter {
            custom,
            options: AnsiOptions {
                width: None,
                hyperlinks: true,
                color: ColorDepth::Ansi256,
            },
        }
    }
}

impl<R> AnsiAdapter<R> {
    pub fn with_options(mut self, options: AnsiOptions) -> Self {
        self.options = options;
        self
    }

    pub fn render<Ext>(&self, item: StreamItem<Node<Ext>>) -> Result<StreamItem<String>, AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        match item {
            StreamItem::Upsert(emission) => Ok(StreamItem::Upsert(Emission {
                id: emission.id,
                provisional: emission.provisional,
                node: self.render_node(&emission.node)?,
            })),
            StreamItem::Delete { id } => Ok(StreamItem::Delete { id }),
        }
    }

    pub fn render_node<Ext>(&self, node: &Node<Ext>) -> Result<String, AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let mut out = String::new();
        self.write_block(node, &mut out)?;
        Ok(out)
    }

    pub fn render_many<Ext>(&self, nodes: &[Node<Ext>]) -> Result<String, AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let mut out = String::new();
        for n in nodes {
            self.write_block(n, &mut out)?;
        }
        Ok(out)
    }

    fn write_block<Ext>(&self, node: &Node<Ext>, out: &mut String) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        match node {
            Node::Document { children, .. } => {
                for child in children {
                    self.write_block(child, out)?;
                }
            }
            Node::Paragraph { children, .. } => {
                let mut buf = String::new();
                self.write_inlines(children, &mut buf)?;
                if let Some(w) = self.options.width {
                    write_wrapped(&buf, 0, w, out);
                } else {
                    out.push_str(&buf);
                }
                out.push_str("\n\n");
            }
            Node::Heading {
                level, children, ..
            } => {
                let style = match level {
                    1 => "\x1b[1;36m",
                    2 => "\x1b[1;35m",
                    _ => "\x1b[1m",
                };
                let mut text = String::new();
                self.write_inlines(children, &mut text)?;
                out.push_str(style);
                out.push_str(&text);
                out.push_str("\x1b[0m\n");
                if *level == 1 {
                    let width = self
                        .options
                        .width
                        .unwrap_or_else(|| visible_len(&text).max(1));
                    let bar_width = visible_len(&text).min(width);
                    out.push_str("\x1b[1;36m");
                    for _ in 0..bar_width {
                        out.push('━');
                    }
                    out.push_str("\x1b[0m\n");
                }
                out.push('\n');
            }
            Node::BlockQuote { children, .. } => {
                let mut buf = String::new();
                for child in children {
                    self.write_block(child, &mut buf)?;
                }
                for line in buf.split_inclusive('\n') {
                    if line == "\n" {
                        out.push_str("\x1b[2m│\x1b[0m\n");
                    } else {
                        out.push_str("\x1b[2m│\x1b[0m ");
                        out.push_str(line);
                    }
                }
            }
            Node::List {
                ordered,
                start,
                children,
                ..
            } => {
                let start = start.unwrap_or(1);
                let mut count = start;
                for child in children {
                    let prefix = if *ordered {
                        let p = format!("  {count}. ");
                        count += 1;
                        p
                    } else {
                        "  • ".to_string()
                    };
                    let mut body = String::new();
                    match child {
                        Node::Item { children, .. } | Node::TaskItem { children, .. } => {
                            for grand in children {
                                self.write_block(grand, &mut body)?;
                            }
                        }
                        other => self.write_block(other, &mut body)?,
                    }
                    let mut first = true;
                    for line in body.split_inclusive('\n') {
                        if first {
                            out.push_str(&prefix);
                            first = false;
                        } else if line != "\n" {
                            for _ in 0..visible_len(&prefix) {
                                out.push(' ');
                            }
                        }
                        out.push_str(line);
                    }
                }
                out.push('\n');
            }
            Node::Item { children, .. } | Node::TaskItem { children, .. } => {
                // Reachable if callers hand us a bare Item. List handler
                // above normally consumes these.
                for child in children {
                    self.write_block(child, out)?;
                }
            }
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => {
                self.write_table(alignments, header, rows, out)?;
            }
            Node::TableRow { .. } | Node::TableCell { .. } => {
                // Handled via the parent Table node only.
            }
            // Three-way dispatch on the first word of the info string.
            // All three forms guarantee a trailing newline plus a blank
            // separator line.
            // [spec:mw:req:ansi.adapter.code-block]
            Node::CodeBlock { info, literal, .. } => {
                let lang = info
                    .as_deref()
                    .and_then(|s| s.split_whitespace().next())
                    .filter(|s| !s.is_empty());
                if matches!(lang, Some("ansi")) {
                    // Content is already styled — emit verbatim and
                    // guarantee a reset follows.
                    out.push_str(literal);
                    if !literal.ends_with("\x1b[0m") {
                        out.push_str("\x1b[0m");
                    }
                    if !literal.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push('\n');
                } else if matches!(lang, Some("terminal")) {
                    // Run through the VT100 emulator and re-emit the
                    // final visible screen as styled ANSI. Drops the
                    // literal cursor-traffic bytes entirely.
                    #[cfg(feature = "terminal")]
                    {
                        let grid = crate::terminal::ansi_snapshot_grid(literal, None, None);
                        out.push_str(&crate::terminal::render_grid_ansi(&grid));
                        if !out.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push('\n');
                    }
                    #[cfg(not(feature = "terminal"))]
                    {
                        out.push_str(literal);
                        if !literal.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push('\n');
                    }
                } else {
                    if let Some(lang) = lang {
                        out.push_str("\x1b[2m");
                        out.push_str(lang);
                        out.push_str("\x1b[0m\n");
                    }
                    for line in literal.split_inclusive('\n') {
                        out.push_str("\x1b[48;5;236m  ");
                        let needs_nl = line.ends_with('\n');
                        let body = if needs_nl {
                            &line[..line.len() - 1]
                        } else {
                            line
                        };
                        out.push_str(body);
                        out.push_str("\x1b[0m");
                        if needs_nl {
                            out.push('\n');
                        }
                    }
                    if !literal.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push('\n');
                }
            }
            Node::MathBlock { math, .. } => {
                out.push_str(math);
                out.push_str("\n\n");
            }
            Node::ThematicBreak { .. } => {
                let width = self.options.width.unwrap_or(60);
                out.push_str("\x1b[2m");
                for _ in 0..width {
                    out.push('─');
                }
                out.push_str("\x1b[0m\n\n");
            }
            Node::Alert {
                kind,
                title,
                children,
                ..
            } => {
                let (color, default_title) = match kind {
                    AlertKind::Note => ("34", "Note"),
                    AlertKind::Tip => ("32", "Tip"),
                    AlertKind::Important => ("33", "Important"),
                    AlertKind::Warning => ("31", "Warning"),
                    AlertKind::Caution => ("91", "Caution"),
                };
                let shown = title.as_deref().unwrap_or(default_title);
                out.push_str("\x1b[1;");
                out.push_str(color);
                out.push('m');
                out.push_str(shown);
                out.push_str("\x1b[0m\n");
                let mut body = String::new();
                for child in children {
                    self.write_block(child, &mut body)?;
                }
                for line in body.split_inclusive('\n') {
                    if line == "\n" {
                        out.push_str("\x1b[");
                        out.push_str(color);
                        out.push_str("m│\x1b[0m\n");
                    } else {
                        out.push_str("\x1b[");
                        out.push_str(color);
                        out.push_str("m│\x1b[0m ");
                        out.push_str(line);
                    }
                }
            }
            Node::DefinitionList { children, .. } => {
                for chunk in children.chunks(2) {
                    if let [
                        Node::DefinitionTerm { children: term, .. },
                        Node::DefinitionDescription { children: desc, .. },
                    ] = chunk
                    {
                        out.push_str("\x1b[1m");
                        self.write_inlines(term, out)?;
                        out.push_str("\x1b[0m\n");
                        for d in desc {
                            let mut body = String::new();
                            self.write_block(d, &mut body)?;
                            for line in body.split_inclusive('\n') {
                                if line != "\n" {
                                    out.push_str("  ");
                                }
                                out.push_str(line);
                            }
                        }
                    }
                }
                out.push('\n');
            }
            Node::DefinitionTerm { .. } | Node::DefinitionDescription { .. } => {
                // Handled by the DefinitionList parent only.
            }
            Node::Html { .. } => {
                // Drop HTML — no reasonable rendering in a terminal.
            }
            Node::FootnoteSection { children, .. } => {
                if !children.is_empty() {
                    out.push_str("\x1b[2m---\x1b[0m\n");
                    for child in children {
                        self.write_block(child, out)?;
                    }
                }
            }
            Node::FootnoteDefinition {
                label, children, ..
            } => {
                out.push_str("\x1b[2m[^");
                out.push_str(label);
                out.push_str("]\x1b[0m ");
                for child in children {
                    self.write_block(child, out)?;
                }
            }
            Node::Custom {
                payload, children, ..
            } => {
                let walk_block = |node: &Node<Ext>, o: &mut String| self.write_block(node, o);
                let walk_inlines =
                    |nodes: &[Node<Ext>], o: &mut String| self.write_inlines(nodes, o);
                let walker = AnsiWalker {
                    walk_block: &walk_block,
                    walk_inlines: &walk_inlines,
                };
                self.custom.render_custom(payload, children, out, &walker)?;
            }
            // Leaves at block level pass through as inlines (e.g. a bare
            // Text at document root).
            Node::Text { .. }
            | Node::CodeSpan { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Subscript { .. }
            | Node::Superscript { .. }
            | Node::Highlight { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::MathInline { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::FootnoteRef { .. } => {
                self.write_inline(node, out)?;
            }
        }
        Ok(())
    }

    fn write_inlines<Ext>(&self, nodes: &[Node<Ext>], out: &mut String) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        for n in nodes {
            self.write_inline(n, out)?;
        }
        Ok(())
    }

    // Emphasis / strong / strikethrough close with their *specific*
    // disable codes (23 / 22 / 29) so an enclosing style survives the
    // inner run; `ESC[0m` there would strip it. Runs that set a
    // background or combine attributes close with `ESC[0m` and so
    // cannot be relied on to preserve an enclosing style. Note 22
    // clears dim as well as bold.
    // [spec:mw:req:ansi.adapter.reset-discipline]
    fn write_inline<Ext>(&self, node: &Node<Ext>, out: &mut String) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        match node {
            Node::Text { text, .. } => out.push_str(text),
            Node::Emphasis { children, .. } => {
                out.push_str("\x1b[3m");
                self.write_inlines(children, out)?;
                out.push_str("\x1b[23m");
            }
            Node::Strong { children, .. } => {
                out.push_str("\x1b[1m");
                self.write_inlines(children, out)?;
                out.push_str("\x1b[22m");
            }
            Node::Strikethrough { children, .. } => {
                out.push_str("\x1b[9m");
                self.write_inlines(children, out)?;
                out.push_str("\x1b[29m");
            }
            Node::Subscript { children, .. } => {
                out.push('_');
                out.push('{');
                self.write_inlines(children, out)?;
                out.push('}');
            }
            Node::Superscript { children, .. } => {
                out.push('^');
                out.push('{');
                self.write_inlines(children, out)?;
                out.push('}');
            }
            Node::Highlight { children, .. } => {
                out.push_str("\x1b[43;30m");
                self.write_inlines(children, out)?;
                out.push_str("\x1b[0m");
            }
            // ST-terminated, not BEL-terminated. `destination` is
            // written verbatim in both forms — neither escaped nor
            // validated, so an ESC in it would end the sequence early.
            // [spec:mw:req:ansi.adapter.links]
            Node::Link {
                destination,
                children,
                ..
            } => {
                if self.options.hyperlinks {
                    out.push_str("\x1b]8;;");
                    out.push_str(destination);
                    out.push_str("\x1b\\");
                    self.write_inlines(children, out)?;
                    out.push_str("\x1b]8;;\x1b\\");
                } else {
                    self.write_inlines(children, out)?;
                    out.push_str(" (");
                    out.push_str(destination);
                    out.push(')');
                }
            }
            Node::Image { alt, .. } => {
                out.push_str("[img: ");
                out.push_str(alt);
                out.push(']');
            }
            Node::CodeSpan { code, .. } => {
                out.push_str("\x1b[48;5;237m");
                out.push_str(code);
                out.push_str("\x1b[0m");
            }
            Node::MathInline { math, .. } => {
                out.push('$');
                out.push_str(math);
                out.push('$');
            }
            Node::SoftBreak { .. } => out.push(' '),
            Node::HardBreak { .. } => out.push('\n'),
            Node::FootnoteRef { number, .. } => {
                out.push_str("\x1b[2m[");
                out.push_str(&number.to_string());
                out.push_str("]\x1b[0m");
            }
            Node::Html { .. } => {
                // Drop.
            }
            Node::Custom {
                payload, children, ..
            } => {
                let walk_block = |node: &Node<Ext>, o: &mut String| self.write_block(node, o);
                let walk_inlines =
                    |nodes: &[Node<Ext>], o: &mut String| self.write_inlines(nodes, o);
                let walker = AnsiWalker {
                    walk_block: &walk_block,
                    walk_inlines: &walk_inlines,
                };
                self.custom.render_custom(payload, children, out, &walker)?;
            }
            Node::Paragraph { .. }
            | Node::Heading { .. }
            | Node::BlockQuote { .. }
            | Node::List { .. }
            | Node::Item { .. }
            | Node::TaskItem { .. }
            | Node::Table { .. }
            | Node::TableRow { .. }
            | Node::TableCell { .. }
            | Node::CodeBlock { .. }
            | Node::MathBlock { .. }
            | Node::ThematicBreak { .. }
            | Node::Alert { .. }
            | Node::DefinitionList { .. }
            | Node::DefinitionTerm { .. }
            | Node::DefinitionDescription { .. }
            | Node::FootnoteDefinition { .. }
            | Node::FootnoteSection { .. }
            | Node::Document { .. } => self.write_block(node, out)?,
        }
        Ok(())
    }

    fn write_table<Ext>(
        &self,
        alignments: &[Option<TableAlignment>],
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        // Very small table renderer — one-pass rendering of cell text,
        // column widths from visible length, ASCII box-drawing. No
        // multi-line cells.
        let mut cells: Vec<Vec<String>> = Vec::new();
        for row in header.iter().chain(rows.iter()) {
            if let Node::TableRow {
                cells: row_cells, ..
            } = row
            {
                let mut line = Vec::with_capacity(row_cells.len());
                for cell in row_cells {
                    if let Node::TableCell { children, .. } = cell {
                        let mut text = String::new();
                        self.write_inlines(children, &mut text)?;
                        line.push(text);
                    }
                }
                cells.push(line);
            }
        }
        if cells.is_empty() {
            return Ok(());
        }
        let columns = cells.iter().map(|c| c.len()).max().unwrap_or(1);
        let mut widths = vec![0usize; columns];
        for row in &cells {
            for (i, text) in row.iter().enumerate() {
                widths[i] = widths[i].max(visible_len(text));
            }
        }

        let draw_border = |out: &mut String, left: char, mid: char, right: char, fill: char| {
            out.push(left);
            for (i, w) in widths.iter().enumerate() {
                for _ in 0..w + 2 {
                    out.push(fill);
                }
                if i + 1 < widths.len() {
                    out.push(mid);
                }
            }
            out.push(right);
            out.push('\n');
        };
        let draw_row = |out: &mut String, line: &[String]| {
            out.push('│');
            for (i, w) in widths.iter().enumerate() {
                out.push(' ');
                let cell = line.get(i).map(|s| s.as_str()).unwrap_or("");
                out.push_str(cell);
                let pad = w.saturating_sub(visible_len(cell));
                let align = alignments.get(i).copied().flatten();
                match align {
                    Some(TableAlignment::Right) => {}
                    _ => {
                        for _ in 0..pad {
                            out.push(' ');
                        }
                    }
                }
                out.push(' ');
                out.push('│');
            }
            out.push('\n');
        };

        draw_border(out, '┌', '┬', '┐', '─');
        if !header.is_empty() {
            draw_row(out, &cells[0]);
            draw_border(out, '├', '┼', '┤', '─');
            for line in &cells[1..] {
                draw_row(out, line);
            }
        } else {
            for line in &cells {
                draw_row(out, line);
            }
        }
        draw_border(out, '└', '┴', '┘', '─');
        out.push('\n');
        Ok(())
    }
}

/// Streaming sink wrapper (symmetrical with `HtmlSink` / `TypstSink`).
// [spec:mw:def:ansi.adapter.surface]
pub struct AnsiSink<S, R = ()> {
    inner: S,
    adapter: AnsiAdapter<R>,
}

impl<S> AnsiSink<S, ()> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            adapter: AnsiAdapter::new(),
        }
    }
}

impl<S, R> AnsiSink<S, R> {
    pub fn with_custom_renderer(inner: S, custom: R) -> Self {
        Self {
            inner,
            adapter: AnsiAdapter {
                custom,
                options: AnsiOptions {
                    width: None,
                    hyperlinks: true,
                    color: ColorDepth::Ansi256,
                },
            },
        }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, R, Ext> Sink<Node<Ext>> for AnsiSink<S, R>
where
    S: Sink<String>,
    S::Error: fmt::Display,
    R: RenderAnsiCustom<Ext>,
{
    type Error = AnsiError;

    fn emit(&mut self, item: StreamItem<Node<Ext>>) -> Result<(), Self::Error> {
        let rendered = self.adapter.render(item)?;
        self.inner
            .emit(rendered)
            .map_err(|e| AnsiError::Sink(e.to_string()))
    }
}

// --- helpers ----------------------------------------------------------

/// Visible column length — strips ANSI escapes and counts UTF-8 chars.
/// Good enough for wrapping / column-width arithmetic; doesn't do
/// wcwidth (treats each char as width 1).
// [spec:mw:req:ansi.adapter.wrapping]
fn visible_len(s: &str) -> usize {
    let mut count = 0usize;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip an escape sequence. CSI: `ESC [ …<final>`. OSC:
            // `ESC ] … ST`.
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC is terminated by ST (`ESC \`) or BEL.
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        count += 1;
    }
    count
}

/// Word-wrap `input` to `width` columns, re-emitting any active style
/// at the start of the continuation line. The algorithm is deliberately
/// simple: split on whitespace, honor hard-break newlines verbatim, and
/// don't try to break very long words.
// Over-long words must not be broken: splitting mid-token could sever
// an escape sequence. Escapes cost zero columns and ride along with
// their word, so a wrapped line may carry a style opened on a previous
// one. Splitting on whitespace collapses space runs and drops leading
// whitespace.
// [spec:mw:req:ansi.adapter.wrapping]
fn write_wrapped(input: &str, initial_col: usize, width: usize, out: &mut String) {
    if width == 0 {
        out.push_str(input);
        return;
    }
    let mut col = initial_col;
    for line in input.split_inclusive('\n') {
        let (body, trailing_nl) = if let Some(stripped) = line.strip_suffix('\n') {
            (stripped, true)
        } else {
            (line, false)
        };
        let mut first_word = col == 0;
        for word in body.split_whitespace() {
            let wlen = visible_len(word);
            if !first_word {
                if col + 1 + wlen > width {
                    out.push('\n');
                    col = 0;
                } else {
                    out.push(' ');
                    col += 1;
                }
            }
            out.push_str(word);
            col += wlen;
            first_word = false;
        }
        if trailing_nl {
            out.push('\n');
            col = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Node, SourceSpan};

    fn render<Ext>(node: Node<Ext>) -> String
    where
        (): RenderAnsiCustom<Ext>,
    {
        AnsiAdapter::new().render_node(&node).unwrap()
    }

    fn txt(s: &str) -> Node<()> {
        Node::Text {
            text: s.into(),
            source: SourceSpan::default(),
        }
    }

    #[test]
    fn heading_gets_bold_color() {
        let node: Node<()> = Node::Heading {
            level: 1,
            children: vec![txt("Hello")],
            source: SourceSpan::default(),
        };
        let out = render(node);
        assert!(out.contains("\x1b[1;36m"));
        assert!(out.contains("Hello"));
        assert!(out.contains("━"));
    }

    // [spec:mw:req:ansi.adapter.reset-discipline/test]
    #[test]
    fn emphasis_and_strong_wrap_content() {
        let para: Node<()> = Node::Paragraph {
            children: vec![
                Node::Emphasis {
                    children: vec![txt("italic")],
                    source: SourceSpan::default(),
                },
                txt(" "),
                Node::Strong {
                    children: vec![txt("bold")],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        };
        let out = render(para);
        assert!(out.contains("\x1b[3mitalic\x1b[23m"));
        assert!(out.contains("\x1b[1mbold\x1b[22m"));
    }

    // [spec:mw:req:ansi.adapter.links/test]
    #[test]
    fn link_emits_osc8_by_default() {
        let link: Node<()> = Node::Link {
            destination: "https://silo.dev".into(),
            title: None,
            children: vec![txt("site")],
            source: SourceSpan::default(),
        };
        let out = render(link);
        assert!(out.contains("\x1b]8;;https://silo.dev\x1b\\"));
        assert!(out.contains("site"));
        assert!(out.contains("\x1b]8;;\x1b\\"));
    }

    // [spec:mw:req:ansi.adapter.code-block/test]
    #[test]
    fn ansi_code_block_passes_through_verbatim() {
        let block: Node<()> = Node::CodeBlock {
            info: Some("ansi".into()),
            literal: "\x1b[31mred\x1b[0m\n".into(),
            source: SourceSpan::default(),
        };
        let out = render(block);
        assert!(out.contains("\x1b[31mred\x1b[0m"));
    }

    // [spec:mw:req:ansi.adapter.code-block/test]
    #[test]
    fn non_ansi_code_block_indents_with_bg() {
        let block: Node<()> = Node::CodeBlock {
            info: Some("rust".into()),
            literal: "fn main() {}\n".into(),
            source: SourceSpan::default(),
        };
        let out = render(block);
        assert!(out.contains("\x1b[2mrust\x1b[0m"));
        assert!(out.contains("\x1b[48;5;236m"));
        assert!(out.contains("fn main"));
    }

    #[test]
    fn alert_title_and_bar() {
        let alert: Node<()> = Node::Alert {
            kind: AlertKind::Warning,
            title: Some("Watch out".into()),
            children: vec![Node::Paragraph {
                children: vec![txt("careful")],
                source: SourceSpan::default(),
            }],
            source: SourceSpan::default(),
        };
        let out = render(alert);
        assert!(out.contains("\x1b[1;31mWatch out\x1b[0m"));
        assert!(out.contains("│"));
        assert!(out.contains("careful"));
    }

    #[test]
    fn ordered_list_uses_numeric_prefix_with_start() {
        let list: Node<()> = Node::List {
            ordered: true,
            start: Some(3),
            tight: Some(true),
            children: vec![
                Node::Item {
                    children: vec![Node::Paragraph {
                        children: vec![txt("three")],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
                Node::Item {
                    children: vec![Node::Paragraph {
                        children: vec![txt("four")],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        };
        let out = render(list);
        assert!(out.contains("3. three"));
        assert!(out.contains("4. four"));
    }

    // [spec:mw:req:ansi.adapter.wrapping/test]
    // [spec:mw:def:ansi.adapter.surface/test]
    #[test]
    fn wrap_respects_width_option() {
        let para: Node<()> = Node::Paragraph {
            children: vec![txt("one two three four five six seven eight nine ten")],
            source: SourceSpan::default(),
        };
        let out = AnsiAdapter::new()
            .with_options(AnsiOptions {
                width: Some(14),
                hyperlinks: true,
                color: ColorDepth::Ansi256,
            })
            .render_node(&para)
            .unwrap();
        // Should not be a single long line.
        let first_line = out.lines().next().unwrap();
        assert!(first_line.chars().count() <= 14, "got `{first_line}`");
    }
}
