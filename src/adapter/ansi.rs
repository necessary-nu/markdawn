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

use crate::ansi_style::{Color, ansi256_to_named, color_to_sgr, rgb_to_ansi256, rgb_to_named};
use crate::node::{Emission, Node, StreamItem};
use crate::sink::Sink;

#[path = "ansi_block.rs"]
mod block;

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
// Every colour the render path emits goes through `downgrade` first, so
// the adapter's own palette-index backgrounds collapse to named entries
// under `Basic16` and a `terminal` transcript's truecolour cells are
// quantised before they are re-encoded.
// [spec:mw:req:ansi.adapter.color-depth+1]
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

/// Quantise `color` down to what `depth` can actually show.
// Truecolour reaches 16 colours in one step rather than by way of the
// 256 cube: routing through the intermediate would quantise twice and
// land further from the requested hue.
// [spec:mw:req:ansi.adapter.color-depth+1]
fn downgrade(color: Color, depth: ColorDepth) -> Color {
    match depth {
        ColorDepth::TrueColor => color,
        ColorDepth::Ansi256 => match color {
            Color::Rgb(r, g, b) => Color::Palette(rgb_to_ansi256(r, g, b)),
            other => other,
        },
        ColorDepth::Basic16 => match color {
            Color::Rgb(r, g, b) => Color::Named(rgb_to_named(r, g, b)),
            Color::Palette(n) => Color::Named(ansi256_to_named(n)),
            other => other,
        },
    }
}

impl<R> AnsiAdapter<R> {
    pub fn with_options(mut self, options: AnsiOptions) -> Self {
        self.options = options;
        self
    }

    /// SGR parameter text for `color` at the configured depth.
    // [spec:mw:req:ansi.adapter.color-depth+1]
    fn sgr_color(&self, color: Color, bg: bool) -> String {
        color_to_sgr(downgrade(color, self.options.color), bg)
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
                out.push_str("\x1b[");
                out.push_str(&self.sgr_color(Color::Named(3), true));
                out.push(';');
                out.push_str(&self.sgr_color(Color::Named(0), false));
                out.push('m');
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
                out.push_str("\x1b[");
                out.push_str(&self.sgr_color(Color::Palette(237), true));
                out.push('m');
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
            } => self.write_custom(payload, children, out)?,
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
    use crate::node::{AlertKind, Node, SourceSpan};

    fn render<Ext>(node: Node<Ext>) -> String
    where
        (): RenderAnsiCustom<Ext>,
    {
        AnsiAdapter::new().render_node(&node).unwrap()
    }

    fn render_at(node: Node<()>, color: ColorDepth) -> String {
        AnsiAdapter::new()
            .with_options(AnsiOptions {
                width: None,
                hyperlinks: true,
                color,
            })
            .render_node(&node)
            .unwrap()
    }

    fn code_block(info: &str, literal: &str) -> Node<()> {
        Node::CodeBlock {
            info: Some(info.into()),
            literal: literal.into(),
            source: SourceSpan::default(),
        }
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

    // [spec:mw:req:ansi.adapter.color-depth+1/test]
    #[test]
    fn downgrade_maps_each_depth() {
        for (depth, expected) in [
            (ColorDepth::TrueColor, Color::Rgb(255, 100, 50)),
            (ColorDepth::Ansi256, Color::Palette(203)),
            (ColorDepth::Basic16, Color::Named(9)),
        ] {
            assert_eq!(downgrade(Color::Rgb(255, 100, 50), depth), expected);
        }
        // A palette index survives 256 but not 16.
        assert_eq!(
            downgrade(Color::Palette(236), ColorDepth::Ansi256),
            Color::Palette(236)
        );
        assert_eq!(
            downgrade(Color::Palette(236), ColorDepth::Basic16),
            Color::Named(0)
        );
        // Named entries are already 16-colour and never move.
        for depth in [
            ColorDepth::TrueColor,
            ColorDepth::Ansi256,
            ColorDepth::Basic16,
        ] {
            assert_eq!(downgrade(Color::Named(6), depth), Color::Named(6));
            assert_eq!(downgrade(Color::Named(15), depth), Color::Named(15));
        }
    }

    // [spec:mw:req:ansi.adapter.color-depth+1/test]
    #[test]
    fn basic16_reduces_the_code_block_band() {
        let block = code_block("rust", "fn main() {}\n");
        for depth in [ColorDepth::TrueColor, ColorDepth::Ansi256] {
            assert!(render_at(block.clone(), depth).contains("\x1b[48;5;236m"));
        }
        let reduced = render_at(block, ColorDepth::Basic16);
        assert!(
            reduced.contains("\x1b[40m") && !reduced.contains("48;5;"),
            "expected a named background at Basic16; got {reduced:?}"
        );
    }

    // [spec:mw:req:ansi.adapter.color-depth+1/test]
    #[test]
    fn basic16_reduces_the_code_span_background() {
        let span: Node<()> = Node::CodeSpan {
            code: "x".into(),
            source: SourceSpan::default(),
        };
        assert!(render_at(span.clone(), ColorDepth::Ansi256).contains("\x1b[48;5;237m"));
        assert!(render_at(span, ColorDepth::Basic16).contains("\x1b[40m"));
    }

    // [spec:mw:req:ansi.adapter.color-depth+1/test]
    #[test]
    fn named_runs_are_identical_at_every_depth() {
        let heading: Node<()> = Node::Heading {
            level: 1,
            children: vec![txt("Hello")],
            source: SourceSpan::default(),
        };
        let alert: Node<()> = Node::Alert {
            kind: AlertKind::Caution,
            title: None,
            children: vec![],
            source: SourceSpan::default(),
        };
        for node in [heading, alert] {
            let truecolor = render_at(node.clone(), ColorDepth::TrueColor);
            for depth in [ColorDepth::Ansi256, ColorDepth::Basic16] {
                assert_eq!(render_at(node.clone(), depth), truecolor);
            }
        }
    }

    // [spec:mw:req:ansi.adapter.color-depth+1/test]
    #[cfg(feature = "terminal")]
    #[test]
    fn terminal_transcript_colors_follow_the_depth() {
        let block = code_block("terminal", "\x1b[38;2;255;100;50mhot\x1b[0m");
        let truecolor = render_at(block.clone(), ColorDepth::TrueColor);
        assert!(truecolor.contains("\x1b[38;2;255;100;50m"), "{truecolor:?}");

        let palette = render_at(block.clone(), ColorDepth::Ansi256);
        assert!(palette.contains("\x1b[38;5;203m"), "{palette:?}");
        assert!(!palette.contains("38;2;"));

        let basic = render_at(block, ColorDepth::Basic16);
        assert!(basic.contains("\x1b[91m"), "{basic:?}");
        assert!(!basic.contains("38;5;") && !basic.contains("38;2;"));
    }
}
