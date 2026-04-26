//! Iced adapter — turn parsed markdown into an [`iced::Element`] tree
//! suitable for embedding in any iced application.
//!
//! Targets `iced = "0.14"` from crates.io. (Iced master is currently
//! unreliable to depend on directly — `iced_winit` pins
//! `web-sys = "=0.3.85"` while `winit` requires `^0.3.70` resolved at a
//! later patch, producing an unresolvable version conflict.)
//!
//! ```ignore
//! use iced::widget::container;
//! use markdawn::{IcedOptions, IcedRenderer, IcedUrl};
//!
//! #[derive(Debug, Clone)]
//! enum Message {
//!     FromMarkdown(IcedUrl),
//! }
//!
//! fn view(state: &State) -> iced::Element<'_, Message> {
//!     let renderer = IcedRenderer::default().with_options(IcedOptions::all());
//!     container(renderer.render(&state.source).map(Message::FromMarkdown)).into()
//! }
//! ```
//!
//! The renderer returns `Element<'static, Url>`. Map it to your own
//! message enum with [`Element::map`]; click events on links arrive as
//! [`Url::Link`], and task-list checkbox toggles arrive as
//! [`Url::TaskToggle`].

use core::cell::Cell;

use ::iced::font::Weight;
use ::iced::widget::{
    Column, button, column, container, image as iced_image, row, rule, span, svg as iced_svg, text,
};
use ::iced::{Background, Border, Color, Element, Font, Length, Padding, Pixels, Theme, alignment};

use crate::adapter::iced::selectable::{
    InlineAtom, Selection, selectable_rich_content, selectable_rich_text, selection_group,
};
use crate::node::{AlertKind, Node, TableAlignment};
use crate::parser::Parser;

const MONOSPACE: Font = Font::MONOSPACE;
const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

const LINK_COLOR: Color = Color::from_rgb(0.10, 0.40, 0.85);
const HIGHLIGHT_BG: Color = Color::from_rgb(1.0, 0.95, 0.50);
const CODE_BG: Color = Color::from_rgb(0.95, 0.95, 0.97);
const QUOTE_BAR: Color = Color::from_rgb(0.80, 0.80, 0.82);

/// Parser + renderer configuration. Mirrors the parser feature flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IcedOptions {
    pub gfm: bool,
    pub math: bool,
    pub alerts: bool,
    pub highlights: bool,
    pub definition_lists: bool,
    /// When the `pikchr` feature is enabled, fenced code blocks tagged
    /// `pikchr` are rendered as inline SVG via `pikru`. No-op otherwise.
    #[cfg(feature = "pikchr")]
    pub pikchr: bool,
    /// When the `graphviz` feature is enabled, fenced code blocks tagged
    /// `dot` or `graphviz` are rendered as inline SVG. No-op otherwise.
    #[cfg(feature = "graphviz")]
    pub graphviz: bool,
    /// When the `typst` feature is enabled, fenced code blocks
    /// tagged `typst` are compiled to inline SVG via the bundled Typst
    /// compiler. Useful for syntree (forest replacement), cetz, and
    /// arbitrary Typst graphics. No-op otherwise.
    #[cfg(feature = "typst")]
    pub typst: bool,
}

impl IcedOptions {
    /// Enable everything markdawn ships.
    pub fn all() -> Self {
        Self {
            gfm: true,
            math: true,
            alerts: true,
            highlights: true,
            definition_lists: true,
            #[cfg(feature = "pikchr")]
            pikchr: true,
            #[cfg(feature = "graphviz")]
            graphviz: true,
            #[cfg(feature = "typst")]
            typst: true,
        }
    }
}

/// Events emitted by the rendered widget tree. Use
/// [`Element::map`](::iced::Element::map) to lift these into the host
/// application's message enum.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Url {
    /// A markdown link was clicked. Payload is the destination URL.
    Link(String),
    /// A task-list checkbox was toggled. `id` is a per-document
    /// monotonically-increasing identifier (1-based, document order);
    /// `checked` is the *target* state requested by the user.
    TaskToggle { id: u64, checked: bool },
}

/// Plug-in for rendering [`Node::Custom`] payloads.
pub trait RenderIcedCustom<Ext> {
    fn render_custom(&self, payload: &Ext, children: &[Node<Ext>]) -> Element<'static, Url>;
}

impl<Ext> RenderIcedCustom<Ext> for () {
    fn render_custom(&self, _payload: &Ext, _children: &[Node<Ext>]) -> Element<'static, Url> {
        column![].into()
    }
}

/// Cached output of an SVG-emitting renderer (math, pikchr, graphviz).
/// Either the rendered + sanitized SVG bytes ready to feed
/// `iced::widget::svg::Handle::from_memory`, or a remembered failure
/// (so we don't re-run an expensive renderer on every view() call
/// just to fail the same way).
#[derive(Clone)]
pub(crate) enum CachedSvg {
    Ok {
        svg: Vec<u8>,
        /// Set for math atoms; None for code-block diagrams.
        atom_metrics: Option<MathMetrics>,
    },
    Err(String),
}

#[derive(Clone, Copy)]
pub(crate) struct MathMetrics {
    pub em_width: f32,
    pub em_height: f32,
    pub em_baseline: f32,
}

/// Stateful renderer that bundles parser options, an HTML tag filter, an
/// optional syntax highlighter, and an optional custom-extension renderer.
pub struct Renderer<R = ()> {
    options: IcedOptions,
    custom: R,
    html_filter: crate::HtmlFilter,
    #[cfg(feature = "highlight")]
    highlighter: core::cell::RefCell<Option<Box<dyn crate::highlight::SpanHighlighter>>>,
    code_block_rewriters: Vec<Box<dyn crate::ext::CodeBlockRewriter>>,
    /// Shared cross-block selection. Cloned into every emitted
    /// SelectableRichText so they coordinate drag-select state.
    pub(crate) selection: Selection,
    /// Memoize SVG-rendering results (math, pikchr, graphviz) by a
    /// stable hash of (kind, source). `view()` runs on every iced
    /// Message — without this cache, every scroll wheel tick reruns
    /// pikchr/mathml/graphviz, killing scroll FPS.
    pub(crate) svg_cache: core::cell::RefCell<std::collections::HashMap<u64, CachedSvg>>,
    /// Memoize syntax-highlighter output by (lang, source) — same
    /// reason as svg_cache. arborium's tokenization is expensive and
    /// would otherwise rerun on every scroll event.
    #[cfg(feature = "highlight")]
    pub(crate) highlight_cache:
        core::cell::RefCell<std::collections::HashMap<u64, Vec<arborium::advanced::Span>>>,
}

impl Default for Renderer<()> {
    fn default() -> Self {
        Self::new(())
    }
}

impl<R> Renderer<R> {
    pub fn new(custom: R) -> Self {
        Self {
            options: IcedOptions::default(),
            custom,
            html_filter: crate::HtmlFilter::safe(),
            #[cfg(feature = "highlight")]
            highlighter: core::cell::RefCell::new(None),
            code_block_rewriters: Vec::new(),
            selection: Selection::new(),
            svg_cache: core::cell::RefCell::new(std::collections::HashMap::new()),
            #[cfg(feature = "highlight")]
            highlight_cache: core::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    pub fn with_options(mut self, options: IcedOptions) -> Self {
        self.options = options;
        self
    }

    /// Override the HTML tag filter (default [`crate::HtmlFilter::safe`]).
    pub fn with_html_filter(mut self, filter: crate::HtmlFilter) -> Self {
        self.html_filter = filter;
        self
    }

    /// Install a syntax highlighter for fenced code blocks. Failures fall
    /// back to plain monospace text.
    #[cfg(feature = "highlight")]
    pub fn with_highlighter<H>(self, highlighter: H) -> Self
    where
        H: crate::highlight::SpanHighlighter + 'static,
    {
        *self.highlighter.borrow_mut() = Some(Box::new(highlighter));
        self
    }

    /// Install a code-block rewriter. Rewriters are invoked before the
    /// pikchr / graphviz / highlighter / plain dispatch; the first
    /// rewriter that returns `Some` substitutes `info` + `literal` for
    /// the rest of the pipeline. Call repeatedly to register multiple.
    pub fn with_code_block_rewriter<W>(mut self, rewriter: W) -> Self
    where
        W: crate::ext::CodeBlockRewriter + 'static,
    {
        self.code_block_rewriters.push(Box::new(rewriter));
        self
    }
}

impl<R> Renderer<R>
where
    R: RenderIcedCustom<()>,
{
    /// Parse `source` and render the resulting block list as a single
    /// `Element`. Returns an empty column on parse failure.
    pub fn render(&self, source: &str) -> Element<'static, Url> {
        let mut builder = Parser::<()>::builder();
        if self.options.gfm {
            builder = builder.with_gfm();
        }
        if self.options.math {
            builder = builder.with_math();
        }
        if self.options.alerts {
            builder = builder.with_alerts();
        }
        if self.options.highlights {
            builder = builder.with_highlights();
        }
        if self.options.definition_lists {
            builder = builder.with_definition_lists();
        }
        let mut parser = builder.build();
        let blocks = match parser.parse_full_at(source, 0) {
            Ok(blocks) => blocks,
            Err(_) => return column![].into(),
        };
        let nodes: Vec<Node<()>> = blocks
            .into_iter()
            .map(|b| {
                let mut node = b.node;
                // parse_full_at always finalizes, so `provisional` is
                // never set even when a fenced block is unclosed.
                // Inspect the source slice directly instead.
                crate::clear_unclosed_diagram_info(source, &mut node);
                node
            })
            .collect();
        let task_id = Cell::new(0_u64);
        let mut col = Column::new().spacing(8);
        for node in &nodes {
            col = col.push(self.render_node(node, &task_id));
        }
        // Wrap the whole rendered region so dead-space clicks (between
        // blocks, around list bullets, etc.) can initiate drag-selects.
        selection_group(col, self.selection.clone()).into()
    }
}

impl<R> Renderer<R> {
    /// Render a single pre-parsed node.
    pub fn render_node<Ext>(&self, node: &Node<Ext>, task_id: &Cell<u64>) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        match node {
            Node::Document { children, .. } | Node::DefinitionList { children, .. } => {
                self.render_block_column(children, task_id).into()
            }
            Node::FootnoteSection { children, .. } => {
                self.render_footnote_section(children, task_id)
            }
            Node::Paragraph { children, .. } => self.render_paragraph(children),
            Node::Heading {
                level, children, ..
            } => self.render_heading(*level, children),
            Node::BlockQuote { children, .. } => self.render_blockquote(children, task_id),
            Node::Alert {
                kind,
                title,
                children,
                ..
            } => self.render_alert(*kind, title.as_deref(), children, task_id),
            Node::List {
                ordered,
                start,
                tight,
                children,
                ..
            } => self.render_list(*ordered, *start, tight.unwrap_or(true), children, task_id),
            Node::Item { children, .. } => self.render_item(children, false, false, true, task_id),
            Node::TaskItem {
                checked, children, ..
            } => self.render_item(children, true, *checked, true, task_id),
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => self.render_table(alignments, header, rows, task_id),
            Node::TableRow { cells, .. } => self.render_table_row(cells, false, &[], task_id),
            Node::TableCell {
                children,
                header,
                alignment,
                ..
            } => self.render_table_cell(*header, *alignment, children),
            Node::CodeBlock { info, literal, .. } => {
                self.render_code_block(info.as_deref(), literal)
            }
            Node::ThematicBreak { .. } => rule::horizontal(1).into(),
            Node::Html { html, .. } => {
                let safe = self.html_filter.filter(html);
                container(text(safe).font(MONOSPACE)).into()
            }
            Node::Text { text: t, .. } => text(t.clone()).into(),
            Node::Emphasis { children, .. } => self.render_paragraph(children),
            Node::Strong { children, .. } => self.render_paragraph(children),
            Node::Strikethrough { children, .. } => self.render_paragraph(children),
            Node::Highlight { children, .. } => self.render_paragraph(children),
            Node::Subscript { children, .. } => self.render_paragraph(children),
            Node::Superscript { children, .. } => self.render_paragraph(children),
            Node::Link { children, .. } => self.render_paragraph(children),
            Node::Image {
                destination,
                alt,
                title,
                ..
            } => render_image(destination, title.as_deref(), alt),
            Node::CodeSpan { code, .. } => text(code.clone()).font(MONOSPACE).into(),
            Node::MathInline { math, .. } => render_math(math, false),
            Node::MathBlock { math, .. } => render_math(math, true),
            Node::SoftBreak { .. } => text(" ").into(),
            Node::HardBreak { .. } => text("\n").into(),
            Node::FootnoteRef {
                label,
                number,
                ref_num,
                ..
            } => render_footnote_ref(label, *number, *ref_num, self.selection.clone()),
            Node::FootnoteDefinition {
                label,
                number,
                backref_count,
                children,
                ..
            } => self.render_footnote_definition(label, *number, *backref_count, children, task_id),
            Node::DefinitionTerm { children, .. } => {
                let mut spans = Vec::new();
                self.collect_inline_spans(children, &mut spans, BOLD);
                selectable_rich_text(spans, self.selection.clone())
                    .on_link_click(Url::Link as fn(String) -> Url)
                    .into()
            }
            Node::DefinitionDescription { children, .. } => {
                container(self.render_block_column(children, task_id))
                    .padding(Padding::default().left(20))
                    .into()
            }
            Node::Custom {
                payload, children, ..
            } => self.custom.render_custom(payload, children),
        }
    }

    fn render_block_column<Ext>(
        &self,
        children: &[Node<Ext>],
        task_id: &Cell<u64>,
    ) -> Column<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let mut col = Column::new().spacing(6);
        for child in children {
            col = col.push(self.render_node(child, task_id));
        }
        col
    }

    fn render_paragraph<Ext>(&self, children: &[Node<Ext>]) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let mut atoms = Vec::new();
        self.collect_inline_atoms(children, &mut atoms, Font::DEFAULT);
        selectable_rich_content(atoms, self.selection.clone())
            .on_link_click(Url::Link as fn(String) -> Url)
            .into()
    }

    fn render_heading<Ext>(&self, level: u8, children: &[Node<Ext>]) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let size: f32 = match level {
            1 => 32.0,
            2 => 26.0,
            3 => 22.0,
            4 => 18.0,
            5 => 16.0,
            _ => 14.0,
        };
        let mut atoms = Vec::new();
        self.collect_inline_atoms(children, &mut atoms, BOLD);
        let atoms = atoms
            .into_iter()
            .map(|a| match a {
                InlineAtom::Span(s) => InlineAtom::Span(s.size(Pixels(size))),
                other => other,
            })
            .collect::<Vec<_>>();
        selectable_rich_content(atoms, self.selection.clone())
            .on_link_click(Url::Link as fn(String) -> Url)
            .into()
    }

    fn render_blockquote<Ext>(
        &self,
        children: &[Node<Ext>],
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        // Simpler than `row![stripe, body]` — that wrapping pattern
        // suppressed the body column from operate() traversal during
        // selection. Instead use a single container with background +
        // left-padding; the visual cue is subtler but the body widgets
        // are direct children of an unambiguous Container so operate
        // recurses into them cleanly.
        let body = self.render_block_column(children, task_id);
        container(body)
            .padding(Padding::default().left(14).right(8).top(4).bottom(4))
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.93, 0.94, 0.96, 0.6))),
                border: Border {
                    color: QUOTE_BAR,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    }

    fn render_alert<Ext>(
        &self,
        kind: AlertKind,
        custom_title: Option<&str>,
        children: &[Node<Ext>],
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let (default_title, accent) = match kind {
            AlertKind::Note => ("Note", Color::from_rgb(0.10, 0.40, 0.85)),
            AlertKind::Tip => ("Tip", Color::from_rgb(0.10, 0.65, 0.30)),
            AlertKind::Important => ("Important", Color::from_rgb(0.55, 0.30, 0.85)),
            AlertKind::Warning => ("Warning", Color::from_rgb(0.85, 0.65, 0.10)),
            AlertKind::Caution => ("Caution", Color::from_rgb(0.85, 0.20, 0.20)),
        };
        let title_text = custom_title.unwrap_or(default_title).to_string();
        let header = selectable_rich_text(
            vec![span(title_text).font(BOLD).color(accent)],
            self.selection.clone(),
        );
        let body = self.render_block_column(children, task_id);
        let inner = Column::new().spacing(4).push(header).push(body);
        container(inner)
            .padding(Padding::default().left(12).right(12).top(8).bottom(8))
            .style(move |_theme: &Theme| {
                let mut style = container::Style::default();
                style.border = Border {
                    color: accent,
                    width: 1.0,
                    radius: 4.0.into(),
                };
                style
            })
            .into()
    }

    fn render_list<Ext>(
        &self,
        ordered: bool,
        start: Option<u64>,
        tight: bool,
        children: &[Node<Ext>],
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let start = start.unwrap_or(1);
        let mut col = Column::new().spacing(if tight { 2 } else { 6 });
        let mut idx: u64 = 0;
        for child in children {
            let marker = if ordered {
                format!("{}.", start + idx)
            } else {
                "•".to_string()
            };
            let item = match child {
                Node::Item { children, .. } => {
                    self.render_item(children, false, false, tight, task_id)
                }
                Node::TaskItem {
                    checked, children, ..
                } => self.render_item(children, true, *checked, tight, task_id),
                other => self.render_node(other, task_id),
            };
            let row_widget = row![
                container(text(marker).font(BOLD)).width(Length::Fixed(24.0)),
                item
            ]
            .spacing(4);
            col = col.push(row_widget);
            idx += 1;
        }
        col.into()
    }

    fn render_item<Ext>(
        &self,
        children: &[Node<Ext>],
        is_task: bool,
        checked: bool,
        tight: bool,
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let mut col = Column::new().spacing(if tight { 2 } else { 6 });
        for (i, child) in children.iter().enumerate() {
            if tight {
                if let Node::Paragraph {
                    children: inlines, ..
                } = child
                {
                    if i > 0 {
                        col = col.push(text(" "));
                    }
                    col = col.push(self.render_paragraph(inlines));
                    continue;
                }
            }
            col = col.push(self.render_node(child, task_id));
        }

        if is_task {
            let id = {
                let next = task_id.get() + 1;
                task_id.set(next);
                next
            };
            let target = !checked;
            let toggle = ::iced::widget::checkbox(checked).on_toggle(move |_| Url::TaskToggle {
                id,
                checked: target,
            });
            row![toggle, col]
                .spacing(6)
                .align_y(::iced::Alignment::Center)
                .into()
        } else {
            col.into()
        }
    }

    fn render_table<Ext>(
        &self,
        alignments: &[Option<TableAlignment>],
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let mut col = Column::new().spacing(2);
        for h in header {
            if let Node::TableRow { cells, .. } = h {
                col = col.push(self.render_table_row(cells, true, alignments, task_id));
            }
        }
        if !header.is_empty() {
            col = col.push(rule::horizontal(1));
        }
        for r in rows {
            if let Node::TableRow { cells, .. } = r {
                col = col.push(self.render_table_row(cells, false, alignments, task_id));
            }
        }
        col.into()
    }

    fn render_table_row<Ext>(
        &self,
        cells: &[Node<Ext>],
        header: bool,
        alignments: &[Option<TableAlignment>],
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let _ = task_id;
        let mut r = ::iced::widget::Row::new().spacing(8);
        for (i, c) in cells.iter().enumerate() {
            let alignment = alignments.get(i).copied().flatten();
            if let Node::TableCell {
                children,
                header: h,
                ..
            } = c
            {
                r = r.push(self.render_table_cell(*h || header, alignment, children));
            }
        }
        r.into()
    }

    fn render_table_cell<Ext>(
        &self,
        header: bool,
        alignment: Option<TableAlignment>,
        children: &[Node<Ext>],
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let font = if header { BOLD } else { Font::DEFAULT };
        let mut atoms = Vec::new();
        self.collect_inline_atoms(children, &mut atoms, font);
        let body = selectable_rich_content(atoms, self.selection.clone())
            .on_link_click(Url::Link as fn(String) -> Url);
        let align_x = match alignment {
            Some(TableAlignment::Left) => alignment::Horizontal::Left,
            Some(TableAlignment::Center) => alignment::Horizontal::Center,
            Some(TableAlignment::Right) => alignment::Horizontal::Right,
            None => alignment::Horizontal::Left,
        };
        container(body).width(Length::Fill).align_x(align_x).into()
    }

    fn render_code_block(&self, info: Option<&str>, literal: &str) -> Element<'static, Url> {
        // 0. user-registered rewriters.
        let rewrite_owned;
        let (info, literal) = if self.code_block_rewriters.is_empty() {
            (info, literal)
        } else {
            let mut rewrite: Option<crate::ext::CodeBlockRewrite> = None;
            for r in &self.code_block_rewriters {
                if let Some(rw) = r.rewrite(info, literal) {
                    rewrite = Some(rw);
                    break;
                }
            }
            match rewrite {
                Some(rw) => {
                    rewrite_owned = rw;
                    (
                        rewrite_owned.info.as_deref(),
                        rewrite_owned.literal.as_str(),
                    )
                }
                None => (info, literal),
            }
        };

        #[cfg_attr(
            not(any(
                feature = "pikchr",
                feature = "graphviz",
                feature = "typst",
                feature = "highlight"
            )),
            allow(unused_variables)
        )]
        let info_lang = info.map(|s| s.split_whitespace().next().unwrap_or(""));

        // 1. pikchr (feature-gated). Cached by (kind, source) so
        //    re-rendering the page (e.g. on every scroll event)
        //    doesn't re-run libpikchr.
        #[cfg(feature = "pikchr")]
        if self.options.pikchr && info_lang == Some("pikchr") {
            let key = svg_cache_key("pikchr", literal);
            let cached = self.svg_cache.borrow().get(&key).cloned();
            let entry = match cached {
                Some(c) => c,
                None => {
                    let entry = match crate::pikchr_render::render(literal) {
                        Ok(svg_src) if svg_src.len() <= crate::svg_sanitize::MAX_SVG_BYTES => {
                            let sanitized = crate::svg_sanitize::sanitize_svg(&svg_src);
                            let final_svg = crate::svg_text_to_paths::text_to_paths(&sanitized);
                            CachedSvg::Ok {
                                svg: final_svg.into_bytes(),
                                atom_metrics: None,
                            }
                        }
                        Ok(_) => CachedSvg::Err(format!(
                            "rendered SVG exceeded {} bytes",
                            crate::svg_sanitize::MAX_SVG_BYTES
                        )),
                        Err(e) => CachedSvg::Err(e.to_string()),
                    };
                    self.svg_cache.borrow_mut().insert(key, entry.clone());
                    entry
                }
            };
            return match entry {
                CachedSvg::Ok { svg, .. } => {
                    iced_svg(::iced::widget::svg::Handle::from_memory(svg)).into()
                }
                CachedSvg::Err(msg) => {
                    render_diagram_error("pikchr", &msg, literal, self.selection.clone())
                }
            };
        }

        // 1c. typst (feature-gated). Cached, see pikchr branch.
        #[cfg(feature = "typst")]
        if self.options.typst && info_lang == Some("typst") {
            let key = svg_cache_key("typst", literal);
            let cached = self.svg_cache.borrow().get(&key).cloned();
            let entry = match cached {
                Some(c) => c,
                None => {
                    let entry = match crate::typst::render(literal) {
                        Ok(svg_src) if svg_src.len() <= crate::svg_sanitize::MAX_SVG_BYTES => {
                            let sanitized = crate::svg_sanitize::sanitize_svg(&svg_src);
                            let final_svg = crate::svg_text_to_paths::text_to_paths(&sanitized);
                            CachedSvg::Ok {
                                svg: final_svg.into_bytes(),
                                atom_metrics: None,
                            }
                        }
                        Ok(_) => CachedSvg::Err(format!(
                            "rendered SVG exceeded {} bytes",
                            crate::svg_sanitize::MAX_SVG_BYTES
                        )),
                        Err(e) => CachedSvg::Err(e.to_string()),
                    };
                    self.svg_cache.borrow_mut().insert(key, entry.clone());
                    entry
                }
            };
            return match entry {
                CachedSvg::Ok { svg, .. } => {
                    iced_svg(::iced::widget::svg::Handle::from_memory(svg)).into()
                }
                CachedSvg::Err(msg) => {
                    render_diagram_error("typst", &msg, literal, self.selection.clone())
                }
            };
        }

        // 1b. graphviz (feature-gated). Cached, see pikchr branch.
        #[cfg(feature = "graphviz")]
        if self.options.graphviz && matches!(info_lang, Some("dot") | Some("graphviz")) {
            let key = svg_cache_key("graphviz", literal);
            let cached = self.svg_cache.borrow().get(&key).cloned();
            let entry = match cached {
                Some(c) => c,
                None => {
                    let entry = match markdawn_graphviz::render_dot_to_svg(literal) {
                        Ok(svg_src) if svg_src.len() <= crate::svg_sanitize::MAX_SVG_BYTES => {
                            let sanitized = crate::svg_sanitize::sanitize_svg(&svg_src);
                            let final_svg = crate::svg_text_to_paths::text_to_paths(&sanitized);
                            CachedSvg::Ok {
                                svg: final_svg.into_bytes(),
                                atom_metrics: None,
                            }
                        }
                        Ok(_) => CachedSvg::Err(format!(
                            "rendered SVG exceeded {} bytes",
                            crate::svg_sanitize::MAX_SVG_BYTES
                        )),
                        Err(e) => CachedSvg::Err(e.to_string()),
                    };
                    self.svg_cache.borrow_mut().insert(key, entry.clone());
                    entry
                }
            };
            return match entry {
                CachedSvg::Ok { svg, .. } => {
                    iced_svg(::iced::widget::svg::Handle::from_memory(svg)).into()
                }
                CachedSvg::Err(msg) => {
                    render_diagram_error("graphviz", &msg, literal, self.selection.clone())
                }
            };
        }

        // 2. syntax highlighter (feature-gated). Cached: arborium's
        //    tokenization is the per-render hot path on scroll.
        #[cfg(feature = "highlight")]
        if let Some(lang) = info_lang.filter(|s| !s.is_empty()) {
            let cache_key = svg_cache_key(&format!("hl:{lang}"), literal);
            let cached = self.highlight_cache.borrow().get(&cache_key).cloned();
            let spans_opt = match cached {
                Some(s) => Some(s),
                None => {
                    let mut borrow = self.highlighter.borrow_mut();
                    let computed = borrow.as_mut().and_then(|hl| hl.spans(lang, literal));
                    if let Some(ref s) = computed {
                        self.highlight_cache
                            .borrow_mut()
                            .insert(cache_key, s.clone());
                    }
                    computed
                }
            };
            if let Some(spans) = spans_opt {
                let rich = highlight_to_spans(literal, &spans);
                return container(
                    selectable_rich_text(rich, self.selection.clone())
                        .on_link_click(Url::Link as fn(String) -> Url),
                )
                .padding(Padding::default().left(8).right(8).top(4).bottom(4))
                .style(code_block_style)
                .width(Length::Fill)
                .into();
            }
        }

        // 3. plain fallback — wrap in SelectableRichText so the body
        //    text participates in drag-select / Cmd+C copy.
        let body = selectable_rich_text(
            vec![span(literal.to_string()).font(MONOSPACE)],
            self.selection.clone(),
        );
        container(body)
            .padding(Padding::default().left(8).right(8).top(4).bottom(4))
            .style(code_block_style)
            .width(Length::Fill)
            .into()
    }

    fn render_footnote_section<Ext>(
        &self,
        children: &[Node<Ext>],
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        if children.is_empty() {
            return column![].into();
        }
        let mut col = Column::new().spacing(4);
        col = col.push(rule::horizontal(1));
        col = col.push(selectable_rich_text(
            vec![span("Footnotes".to_string()).font(BOLD).size(Pixels(14.0))],
            self.selection.clone(),
        ));
        for child in children {
            col = col.push(self.render_node(child, task_id));
        }
        col.into()
    }

    fn render_footnote_definition<Ext>(
        &self,
        label: &str,
        number: u32,
        _backref_count: u32,
        children: &[Node<Ext>],
        task_id: &Cell<u64>,
    ) -> Element<'static, Url>
    where
        R: RenderIcedCustom<Ext>,
    {
        let _ = label;
        let prefix = container(text(format!("{number}.")).font(BOLD)).width(Length::Fixed(28.0));
        let body = self.render_block_column(children, task_id);
        row![prefix, body].spacing(4).into()
    }

    fn collect_inline_spans<Ext>(
        &self,
        nodes: &[Node<Ext>],
        out: &mut Vec<::iced::widget::text::Span<'static, String>>,
        base_font: Font,
    ) where
        R: RenderIcedCustom<Ext>,
    {
        // Backwards-compat wrapper for callers that don't need atom
        // support. Math falls back to monospace LaTeX source.
        let mut atoms = Vec::new();
        self.collect_inline_atoms(nodes, &mut atoms, base_font);
        for atom in atoms {
            match atom {
                InlineAtom::Span(s) => out.push(s),
                InlineAtom::Replaced { copy_text, .. } => {
                    out.push(span(copy_text).font(MONOSPACE));
                }
            }
        }
    }

    fn collect_inline_atoms<Ext>(
        &self,
        nodes: &[Node<Ext>],
        out: &mut Vec<InlineAtom<'static>>,
        base_font: Font,
    ) where
        R: RenderIcedCustom<Ext>,
    {
        for node in nodes {
            self.push_inline_atom(node, out, base_font);
        }
    }

    fn push_inline_atom<Ext>(
        &self,
        node: &Node<Ext>,
        out: &mut Vec<InlineAtom<'static>>,
        base_font: Font,
    ) where
        R: RenderIcedCustom<Ext>,
    {
        match node {
            Node::Text { text, .. } => {
                out.push(InlineAtom::Span(span(text.clone()).font(base_font)));
            }
            Node::Emphasis { children, .. } => {
                let next = blend_italic(base_font);
                self.collect_inline_atoms(children, out, next);
            }
            Node::Strong { children, .. } => {
                let next = blend_bold(base_font);
                self.collect_inline_atoms(children, out, next);
            }
            Node::Strikethrough { children, .. } => {
                let mut nested = Vec::new();
                self.collect_inline_atoms(children, &mut nested, base_font);
                for atom in nested {
                    out.push(atom_map_span(atom, |s| s.strikethrough(true)));
                }
            }
            Node::Highlight { children, .. } => {
                let mut nested = Vec::new();
                self.collect_inline_atoms(children, &mut nested, base_font);
                for atom in nested {
                    out.push(atom_map_span(atom, |s| s.background(HIGHLIGHT_BG)));
                }
            }
            Node::Subscript { children, .. } => {
                if let Some(plain) = flatten_to_plain(children) {
                    if let Some(unicode) = to_unicode_subscript(&plain) {
                        out.push(InlineAtom::Span(span(unicode).font(base_font)));
                        return;
                    }
                }
                // Fallback: scaled-down text (no Unicode equivalent
                // for some characters, or children contain nested
                // formatting we can't flatten).
                let mut nested = Vec::new();
                self.collect_inline_atoms(children, &mut nested, base_font);
                for atom in nested {
                    out.push(atom_map_span(atom, |s| s.size(Pixels(11.0))));
                }
            }
            Node::Superscript { children, .. } => {
                if let Some(plain) = flatten_to_plain(children) {
                    if let Some(unicode) = to_unicode_superscript(&plain) {
                        out.push(InlineAtom::Span(span(unicode).font(base_font)));
                        return;
                    }
                }
                let mut nested = Vec::new();
                self.collect_inline_atoms(children, &mut nested, base_font);
                for atom in nested {
                    out.push(atom_map_span(atom, |s| s.size(Pixels(11.0))));
                }
            }
            Node::CodeSpan { code, .. } => {
                out.push(InlineAtom::Span(
                    span(code.clone()).font(MONOSPACE).background(CODE_BG),
                ));
            }
            Node::Link {
                destination,
                children,
                ..
            } => {
                let safe = crate::url_safety::is_safe_url(destination);
                let mut nested = Vec::new();
                self.collect_inline_atoms(children, &mut nested, base_font);
                let link_href = if safe {
                    Some(destination.clone())
                } else {
                    None
                };
                for atom in nested {
                    let atom = atom_apply_link(atom, LINK_COLOR, link_href.clone());
                    out.push(atom_map_span(atom, |s| {
                        let s = s.color(LINK_COLOR).underline(true);
                        if safe { s.link(destination.clone()) } else { s }
                    }));
                }
            }
            Node::Image {
                destination, alt, ..
            } => {
                if let Some(atom) = try_inline_image_atom(destination, alt) {
                    out.push(atom);
                } else if !alt.is_empty() {
                    out.push(InlineAtom::Span(span(alt.clone()).font(base_font)));
                }
            }
            Node::SoftBreak { .. } => {
                out.push(InlineAtom::Span(span(" ").font(base_font)));
            }
            Node::HardBreak { .. } => {
                out.push(InlineAtom::Span(span("\n").font(base_font)));
            }
            Node::FootnoteRef {
                label,
                number,
                ref_num,
                ..
            } => {
                let suffix = if *ref_num > 1 {
                    format!("-{ref_num}")
                } else {
                    String::new()
                };
                let target = format!("#fnref-{label}{suffix}");
                let _ = number;
                out.push(InlineAtom::Span(
                    span(format!("[{number}]"))
                        .font(base_font)
                        .color(LINK_COLOR)
                        .underline(true)
                        .size(Pixels(10.0))
                        .link(target),
                ));
            }
            Node::CodeBlock { literal, .. } => {
                out.push(InlineAtom::Span(span(literal.clone()).font(MONOSPACE)));
            }
            Node::Html { html, .. } => {
                let safe = self.html_filter.filter(html);
                out.push(InlineAtom::Span(span(safe).font(base_font)));
            }
            Node::MathInline { math, .. } | Node::MathBlock { math, .. } => {
                let inline_font_size = 16.0_f64;
                let copy_text = format!("${math}$");
                let key = svg_cache_key("math", math);
                let cached = self.svg_cache.borrow().get(&key).cloned();
                let entry = match cached {
                    Some(c) => c,
                    None => {
                        let entry =
                            match mathml_core::latex_to_svg_with_metrics(math, inline_font_size) {
                                Ok(rendered) => {
                                    let fs = inline_font_size as f32;
                                    let em_width = (rendered.width as f32 / fs).max(0.5);
                                    let em_height = (rendered.height as f32 / fs).max(1.0);
                                    let em_baseline =
                                        (rendered.baseline as f32 / fs).clamp(0.0, em_height);
                                    CachedSvg::Ok {
                                        svg: rendered.svg.into_bytes(),
                                        atom_metrics: Some(MathMetrics {
                                            em_width,
                                            em_height,
                                            em_baseline,
                                        }),
                                    }
                                }
                                Err(e) => CachedSvg::Err(e.to_string()),
                            };
                        self.svg_cache.borrow_mut().insert(key, entry.clone());
                        entry
                    }
                };
                match entry {
                    CachedSvg::Ok {
                        svg,
                        atom_metrics: Some(m),
                    } => {
                        out.push(InlineAtom::Replaced {
                            svg,
                            copy_text,
                            em_width: m.em_width,
                            em_height: m.em_height,
                            em_baseline: m.em_baseline,
                            color: None,
                            link: None,
                        });
                    }
                    _ => {
                        out.push(InlineAtom::Span(span(math.clone()).font(MONOSPACE)));
                    }
                }
            }
            Node::Custom { children, .. } => {
                self.collect_inline_atoms(children, out, base_font);
            }
            _ => {}
        }
    }
}

fn atom_map_span<F>(atom: InlineAtom<'static>, f: F) -> InlineAtom<'static>
where
    F: FnOnce(
        ::iced::widget::text::Span<'static, String>,
    ) -> ::iced::widget::text::Span<'static, String>,
{
    match atom {
        InlineAtom::Span(s) => InlineAtom::Span(f(s)),
        other => other,
    }
}

/// Apply Link styling to a Replaced atom (math SVG): tint the SVG
/// with the link color AND attach the destination URL to its
/// placeholder span so clicks on the rendered SVG fire the link
/// handler. Span variants pass through unchanged — their styling is
/// set via `atom_map_span`.
fn atom_apply_link(
    atom: InlineAtom<'static>,
    color: Color,
    href: Option<String>,
) -> InlineAtom<'static> {
    match atom {
        InlineAtom::Replaced {
            svg,
            copy_text,
            em_width,
            em_height,
            em_baseline,
            color: existing_color,
            link: existing_link,
        } => InlineAtom::Replaced {
            svg,
            copy_text,
            em_width,
            em_height,
            em_baseline,
            color: existing_color.or(Some(color)),
            link: existing_link.or(href),
        },
        other => other,
    }
}

fn blend_bold(base: Font) -> Font {
    Font {
        weight: Weight::Bold,
        ..base
    }
}

fn blend_italic(base: Font) -> Font {
    Font {
        style: ::iced::font::Style::Italic,
        ..base
    }
}

fn code_block_style(_theme: &Theme) -> container::Style {
    let mut style = container::Style::default();
    style.background = Some(Background::Color(CODE_BG));
    style.border = Border {
        color: Color::from_rgb(0.85, 0.85, 0.88),
        width: 1.0,
        radius: 3.0.into(),
    };
    style
}

fn render_image(destination: &str, _title: Option<&str>, alt: &str) -> Element<'static, Url> {
    if !crate::url_safety::is_safe_url(destination) {
        return text(alt.to_string()).into();
    }
    if let Some(bytes) = decode_data_uri(destination) {
        return iced_image(::iced::widget::image::Handle::from_bytes(bytes)).into();
    }
    if !destination.contains("://") {
        return iced_image(::iced::widget::image::Handle::from_path(
            destination.to_string(),
        ))
        .into();
    }
    // Remote image: degrade to alt text. Hosts wanting remote images can
    // pre-fetch and substitute a `Node::Custom` via an extension.
    text(alt.to_string()).into()
}

fn decode_data_uri(url: &str) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("data:")?;
    let (_meta, payload) = rest.split_once(',')?;
    if let Some(b64) = _meta
        .split(';')
        .map(str::trim)
        .find(|s| s.eq_ignore_ascii_case("base64"))
    {
        let _ = b64;
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .ok()
    } else {
        Some(payload.as_bytes().to_vec())
    }
}

/// Render a diagram (pikchr, graphviz) error inline so the user can
/// see WHY the render failed instead of getting a silent fallback to
/// monospace source. Shows a red banner with the engine name and
/// error message, followed by the original source.
#[cfg(any(feature = "pikchr", feature = "graphviz"))]
fn render_diagram_error(
    engine: &str,
    message: &str,
    source: &str,
    selection: Selection,
) -> Element<'static, Url> {
    let header_color = Color::from_rgb(0.78, 0.20, 0.20);
    let body_bg = Color::from_rgba(1.0, 0.92, 0.92, 1.0);
    let header = selectable_rich_text(
        vec![
            span(format!("{engine} error"))
                .font(BOLD)
                .color(header_color),
        ],
        selection.clone(),
    );
    let msg = selectable_rich_text(
        vec![
            span(strip_ansi_escapes_simple(message))
                .font(MONOSPACE)
                .size(Pixels(11.0)),
        ],
        selection.clone(),
    );
    let body = selectable_rich_text(
        vec![span(source.to_string()).font(MONOSPACE).size(Pixels(11.0))],
        selection,
    );
    container(column![header, msg, body].spacing(4))
        .padding(Padding::default().left(8).right(8).top(6).bottom(6))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(body_bg)),
            border: Border {
                color: header_color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .width(Length::Fill)
        .into()
}

/// Try to render an inline `![alt](destination)` image as a real
/// inline replaced atom. Currently only handles `data:image/svg+xml`
/// URIs (the cleanest inline-image form for self-contained markdown).
/// Other schemes (http, file, raster data:) fall back to alt text.
///
/// SVG dimensions come from the SVG's own `width="N"`/`height="N"`
/// attributes; we use those at face value (in pixels) and convert
/// to em units via the demo's body font size of 16. If width/height
/// aren't present, fall back to alt text.
fn try_inline_image_atom(destination: &str, alt: &str) -> Option<InlineAtom<'static>> {
    let prefix_utf8 = "data:image/svg+xml;utf8,";
    let prefix_b64 = "data:image/svg+xml;base64,";
    let prefix_plain = "data:image/svg+xml,";
    let svg_bytes: Vec<u8> = if let Some(rest) = destination.strip_prefix(prefix_utf8) {
        percent_decode(rest).into_bytes()
    } else if let Some(rest) = destination.strip_prefix(prefix_plain) {
        percent_decode(rest).into_bytes()
    } else if let Some(rest) = destination.strip_prefix(prefix_b64) {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(rest.trim())
            .ok()?
    } else {
        return None;
    };
    let (px_w, px_h) = parse_svg_dimensions(&svg_bytes)?;
    const BODY_FONT: f32 = 16.0;
    let em_width = (px_w / BODY_FONT).max(0.5);
    let em_height = (px_h / BODY_FONT).max(0.5);
    Some(InlineAtom::Replaced {
        svg: svg_bytes,
        copy_text: format!("![{alt}]({destination})"),
        em_width,
        em_height,
        em_baseline: em_height, // image baseline = bottom edge
        color: None,
        link: None,
    })
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// Cheap-and-cheerful extraction of `(width, height)` in pixels
/// from the SVG header. Stable enough for self-generated SVGs (math,
/// pikchr, graphviz, demo data URIs) — they all set explicit
/// `width=`/`height=` attributes within the first few hundred bytes.
fn parse_svg_dimensions(svg: &[u8]) -> Option<(f32, f32)> {
    let head_end = 512.min(svg.len());
    let head = std::str::from_utf8(svg.get(..head_end)?).ok()?;
    Some((
        read_attr_f32(head, " width=\"")?,
        read_attr_f32(head, " height=\"")?,
    ))
}

fn read_attr_f32(head: &str, key: &str) -> Option<f32> {
    let i = head.find(key)?;
    let rest = &head[i + key.len()..];
    let end = rest.find('"')?;
    rest[..end]
        .trim()
        .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
        .parse::<f32>()
        .ok()
}

/// Walk `nodes` and concatenate all leaf text, returning `None` if
/// any non-text leaf appears (Strong/Emphasis/Code/etc.). Used to
/// decide whether sub/sup content is plain enough to map to Unicode
/// subscript/superscript characters.
fn flatten_to_plain<Ext>(nodes: &[Node<Ext>]) -> Option<String> {
    let mut s = String::new();
    for node in nodes {
        match node {
            Node::Text { text, .. } => s.push_str(text),
            Node::SoftBreak { .. } => s.push(' '),
            _ => return None,
        }
    }
    Some(s)
}

/// Map a string to its Unicode subscript form, returning `None` if
/// any character has no subscript equivalent. Real Unicode subscript
/// glyphs render at the correct baseline + size for their containing
/// font — no widget tricks needed.
fn to_unicode_subscript(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(match c {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            '+' => '₊',
            '-' => '₋',
            '=' => '₌',
            '(' => '₍',
            ')' => '₎',
            'a' => 'ₐ',
            'e' => 'ₑ',
            'h' => 'ₕ',
            'i' => 'ᵢ',
            'j' => 'ⱼ',
            'k' => 'ₖ',
            'l' => 'ₗ',
            'm' => 'ₘ',
            'n' => 'ₙ',
            'o' => 'ₒ',
            'p' => 'ₚ',
            'r' => 'ᵣ',
            's' => 'ₛ',
            't' => 'ₜ',
            'u' => 'ᵤ',
            'v' => 'ᵥ',
            'x' => 'ₓ',
            ' ' => ' ',
            _ => return None,
        });
    }
    Some(out)
}

/// Map a string to its Unicode superscript form, `None` on miss.
fn to_unicode_superscript(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(match c {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '+' => '⁺',
            '-' => '⁻',
            '=' => '⁼',
            '(' => '⁽',
            ')' => '⁾',
            'a' => 'ᵃ',
            'b' => 'ᵇ',
            'c' => 'ᶜ',
            'd' => 'ᵈ',
            'e' => 'ᵉ',
            'f' => 'ᶠ',
            'g' => 'ᵍ',
            'h' => 'ʰ',
            'i' => 'ⁱ',
            'j' => 'ʲ',
            'k' => 'ᵏ',
            'l' => 'ˡ',
            'm' => 'ᵐ',
            'n' => 'ⁿ',
            'o' => 'ᵒ',
            'p' => 'ᵖ',
            'r' => 'ʳ',
            's' => 'ˢ',
            't' => 'ᵗ',
            'u' => 'ᵘ',
            'v' => 'ᵛ',
            'w' => 'ʷ',
            'x' => 'ˣ',
            'y' => 'ʸ',
            'z' => 'ᶻ',
            'A' => 'ᴬ',
            'B' => 'ᴮ',
            'D' => 'ᴰ',
            'E' => 'ᴱ',
            'G' => 'ᴳ',
            'H' => 'ᴴ',
            'I' => 'ᴵ',
            'J' => 'ᴶ',
            'K' => 'ᴷ',
            'L' => 'ᴸ',
            'M' => 'ᴹ',
            'N' => 'ᴺ',
            'O' => 'ᴼ',
            'P' => 'ᴾ',
            'R' => 'ᴿ',
            'T' => 'ᵀ',
            'U' => 'ᵁ',
            'V' => 'ⱽ',
            'W' => 'ᵂ',
            ' ' => ' ',
            _ => return None,
        });
    }
    Some(out)
}

/// Stable hash of `(kind, source)` used as the SVG cache key.
fn svg_cache_key(kind: &str, source: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut h);
    source.hash(&mut h);
    h.finish()
}

/// Strip ANSI CSI escape sequences (`ESC [ ... letter`) from a
/// string so embedded color/style codes don't render as visible
/// `[31m` garbage in the diagnostic banner. Keep this dependency-
/// free — only handles CSI which is what `pikru` emits.
#[cfg(any(feature = "pikchr", feature = "graphviz"))]
fn strip_ansi_escapes_simple(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI sequence: ESC '[' params... final byte 0x40-0x7E
            if matches!(chars.next(), Some('[')) {
                for term in chars.by_ref() {
                    if matches!(term, '@'..='~') {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn render_math(math: &str, display: bool) -> Element<'static, Url> {
    let size = if display { 22.0 } else { 16.0 };
    match mathml_core::latex_to_svg(math, size) {
        Ok(svg_src) => iced_svg(::iced::widget::svg::Handle::from_memory(
            svg_src.into_bytes(),
        ))
        .into(),
        Err(_) => text(math.to_string()).font(MONOSPACE).into(),
    }
}

fn render_footnote_ref(
    label: &str,
    number: u32,
    ref_num: u32,
    selection: Selection,
) -> Element<'static, Url> {
    let suffix = if ref_num > 1 {
        format!("-{ref_num}")
    } else {
        String::new()
    };
    let target = format!("#fnref-{label}{suffix}");
    let s = span(format!("[{number}]"))
        .color(LINK_COLOR)
        .underline(true)
        .size(Pixels(10.0))
        .link(target);
    selectable_rich_text(vec![s], selection)
        .on_link_click(Url::Link as fn(String) -> Url)
        .into()
}

#[cfg(feature = "highlight")]
fn highlight_to_spans(
    src: &str,
    spans: &[arborium::advanced::Span],
) -> Vec<::iced::widget::text::Span<'static, String>> {
    // arborium emits potentially OVERLAPPING / nested spans (an
    // identifier inside a function-call inside a statement, etc.).
    // The naive sequential walker emitted overlapping bytes multiple
    // times — that's why every token appeared duplicated.
    //
    // Split the source at every span boundary into half-open
    // segments. For each segment, pick the innermost (smallest) span
    // covering it as the color source. Segments not covered by any
    // span render with no color.
    let len = src.len();
    let mut breaks: Vec<usize> = Vec::with_capacity(2 + spans.len() * 2);
    breaks.push(0);
    breaks.push(len);
    for s in spans {
        let lo = (s.start as usize).min(len);
        let hi = (s.end as usize).min(len);
        breaks.push(lo);
        breaks.push(hi);
    }
    breaks.sort_unstable();
    breaks.dedup();

    let mut out = Vec::with_capacity(breaks.len().saturating_sub(1));
    for window in breaks.windows(2) {
        let (lo, hi) = (window[0], window[1]);
        if lo >= hi || hi > len || !src.is_char_boundary(lo) || !src.is_char_boundary(hi) {
            continue;
        }
        // Pick innermost (smallest-range) span covering [lo, hi).
        let mut best: Option<&arborium::advanced::Span> = None;
        for s in spans {
            let s_lo = (s.start as usize).min(len);
            let s_hi = (s.end as usize).min(len);
            if s_lo <= lo && hi <= s_hi {
                match best {
                    None => best = Some(s),
                    Some(b) => {
                        if (s.end - s.start) < (b.end - b.start) {
                            best = Some(s);
                        }
                    }
                }
            }
        }
        let mut sp = span(src[lo..hi].to_string()).font(MONOSPACE);
        if let Some(s) = best {
            if let Some((r, g, b)) = scope_color(&s.capture) {
                sp = sp.color(Color::from_rgb(r, g, b));
            }
        }
        out.push(sp);
    }
    out
}

#[cfg(feature = "highlight")]
fn scope_color(scope: &str) -> Option<(f32, f32, f32)> {
    // Minimal default palette. Hosts wanting fine-grained theming can
    // post-process the output or provide a custom highlighter that emits
    // a finer scope tree.
    let head = scope.split('.').next().unwrap_or(scope);
    Some(match head {
        "keyword" => (0.55, 0.20, 0.65),
        "string" => (0.20, 0.55, 0.30),
        "comment" => (0.45, 0.45, 0.50),
        "constant" | "number" => (0.75, 0.45, 0.10),
        "function" | "entity" => (0.10, 0.40, 0.75),
        "type" | "support" => (0.10, 0.55, 0.55),
        "variable" => (0.35, 0.30, 0.30),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn render_default(src: &str) -> Element<'static, Url> {
        Renderer::default().render(src)
    }

    #[test]
    fn empty_input_does_not_panic() {
        let _ = render_default("");
    }

    #[test]
    fn paragraph_renders() {
        let _ = render_default("hello **world**");
    }

    #[test]
    fn heading_renders() {
        let _ = render_default(
            "# Title\n\n## Sub\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six",
        );
    }

    #[test]
    fn list_with_task_items_renders() {
        let _ = Renderer::default()
            .with_options(IcedOptions {
                gfm: true,
                ..Default::default()
            })
            .render("- one\n- [x] done\n- [ ] todo\n");
    }

    #[test]
    fn ordered_list_renders() {
        let _ = render_default("1. one\n2. two\n3. three");
    }

    #[test]
    fn code_block_renders() {
        let _ = render_default("```rust\nfn main() {}\n```");
    }

    #[test]
    fn blockquote_renders() {
        let _ = render_default("> hello\n> world");
    }

    #[test]
    fn alert_renders() {
        let _ = Renderer::default()
            .with_options(IcedOptions {
                alerts: true,
                ..Default::default()
            })
            .render("> [!WARNING]\n> careful");
    }

    #[test]
    fn table_renders() {
        let _ = Renderer::default()
            .with_options(IcedOptions {
                gfm: true,
                ..Default::default()
            })
            .render("| a | b |\n|:--|---:|\n| 1 | 2 |");
    }

    #[test]
    fn link_renders() {
        let _ = render_default("[click](https://example.com)");
    }

    #[test]
    fn javascript_link_does_not_become_clickable() {
        // Smoke test: should not panic and the unsafe URL should be
        // dropped in `push_inline_span`. We can't introspect the Element
        // directly, but the path covered should match the safe-url guard.
        let _ = render_default("[x](javascript:alert(1))");
    }

    #[test]
    fn math_inline_renders() {
        let _ = Renderer::default()
            .with_options(IcedOptions {
                math: true,
                ..Default::default()
            })
            .render("the formula $a+b$ is simple");
    }

    #[test]
    fn math_block_renders() {
        let _ = Renderer::default()
            .with_options(IcedOptions {
                math: true,
                ..Default::default()
            })
            .render("$$x^2+y^2=z^2$$");
    }

    #[test]
    fn thematic_break_renders() {
        let _ = render_default("---\n");
    }

    #[test]
    fn footnote_renders() {
        let _ = Renderer::default()
            .with_options(IcedOptions {
                gfm: true,
                ..Default::default()
            })
            .render("see[^1]\n\n[^1]: a footnote");
    }

    #[test]
    fn definition_list_renders() {
        let _ = Renderer::default()
            .with_options(IcedOptions {
                definition_lists: true,
                ..Default::default()
            })
            .render("term\n: definition\n");
    }

    #[test]
    fn custom_renderer_is_invoked() {
        use std::cell::Cell as StdCell;
        use std::rc::Rc;

        struct CountCustom(Rc<StdCell<u32>>);
        impl RenderIcedCustom<&'static str> for CountCustom {
            fn render_custom(
                &self,
                _payload: &&'static str,
                _children: &[Node<&'static str>],
            ) -> Element<'static, Url> {
                self.0.set(self.0.get() + 1);
                Column::new().into()
            }
        }

        let counter = Rc::new(StdCell::new(0_u32));
        let renderer = Renderer::new(CountCustom(counter.clone()));
        let task_id = Cell::new(0);
        let node: Node<&'static str> = Node::Custom {
            payload: "test",
            children: vec![],
            source: crate::node::SourceSpan { start: 0, end: 0 },
        };
        let _ = renderer.render_node(&node, &task_id);
        assert_eq!(counter.get(), 1, "render_custom should be called once");
    }

    // ─── Per-variant smoke tests (every Node variant exercised) ──────

    fn render_all() -> impl Fn(&str) -> Element<'static, Url> {
        |src: &str| {
            Renderer::default()
                .with_options(IcedOptions::all())
                .render(src)
        }
    }

    #[test]
    fn strong_emphasis_strikethrough() {
        let r = render_all();
        let _ = r("**bold**");
        let _ = r("*italic*");
        let _ = r("_italic_");
        let _ = r("***bold italic***");
        let _ = r("~~strike~~");
    }

    #[test]
    fn code_span() {
        let _ = render_default("here is `inline code` mid-sentence");
    }

    #[test]
    fn highlight_inline() {
        let r = render_all();
        let _ = r("a ==highlighted== word");
    }

    #[test]
    fn subscript_superscript() {
        let r = render_all();
        let _ = r("H~2~O and E=mc^2^");
    }

    #[test]
    fn soft_and_hard_breaks() {
        // Soft break: trailing newline inside a paragraph.
        let _ = render_default("line one\nline two\n");
        // Hard break: two trailing spaces before newline.
        let _ = render_default("line one  \nline two\n");
        // Backslash hard break (CommonMark).
        let _ = render_default("line one\\\nline two\n");
    }

    #[test]
    fn html_block_filtered_by_default() {
        let _ = render_default("<script>alert('xss')</script>\n\nsafe text");
    }

    #[test]
    fn html_block_unrestricted_does_not_panic() {
        let _ = Renderer::default()
            .with_html_filter(crate::HtmlFilter::unrestricted())
            .render("<div>hello</div>");
    }

    #[test]
    fn image_local_path() {
        let _ = render_default("![alt](some/local/path.png)");
    }

    #[test]
    fn image_remote_url_falls_back_to_alt() {
        let _ = render_default("![alt](https://example.com/x.png)");
    }

    #[test]
    fn image_data_uri_decodes() {
        // 1×1 PNG, base64-encoded.
        let _ = render_default(
            "![pixel](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkAAIAAAoAAv/lxKUAAAAASUVORK5CYII=)",
        );
    }

    #[test]
    fn image_javascript_url_is_stripped() {
        // Should not panic and should degrade to alt text.
        let _ = render_default("![x](javascript:alert(1))");
    }

    #[test]
    fn all_alert_kinds_render() {
        let r = render_all();
        for kind in ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"] {
            let src = format!("> [!{kind}]\n> body for {kind}");
            let _ = r(&src);
        }
    }

    #[test]
    fn alert_with_custom_title() {
        let r = render_all();
        let _ = r("> [!NOTE] Custom Title\n> body");
    }

    #[test]
    fn loose_list_renders() {
        // Blank line between items → loose list (paragraphs preserved).
        let _ = render_default("- one\n\n- two\n\n- three\n");
    }

    #[test]
    fn nested_list_renders() {
        let _ = render_default("- outer\n  - inner\n    - deeper\n- back\n");
    }

    #[test]
    fn empty_table_renders() {
        let r = render_all();
        // Header only, no body rows.
        let _ = r("| a | b |\n|---|---|\n");
    }

    #[test]
    fn table_with_all_alignments() {
        let r = render_all();
        let _ = r("| L | C | R | D |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |");
    }

    #[test]
    fn nested_blockquote_renders() {
        let _ = render_default("> outer\n> > inner\n> back");
    }

    #[test]
    fn blockquote_with_heading_inside() {
        let _ = render_default("> # heading\n> body");
    }

    #[test]
    fn blockquote_with_code_inside() {
        let _ = render_default("> ```rust\n> let x = 1;\n> ```");
    }

    #[test]
    fn link_with_title_renders() {
        let _ = render_default(r#"[click](https://example.com "tooltip")"#);
    }

    #[test]
    fn autolink_renders() {
        let r = render_all();
        let _ = r("see <https://example.com> for details");
    }

    #[test]
    fn data_url_link_is_stripped() {
        let _ = render_default("[x](data:text/html;base64,PHNjcmlwdD4=)");
    }

    #[test]
    fn vbscript_link_is_stripped() {
        let _ = render_default("[x](vbscript:msgbox)");
    }

    #[test]
    fn footnote_section_with_multiple_refs() {
        let r = render_all();
        let _ =
            r("see[^a] and[^b] and[^a] again\n\n[^a]: first footnote\n\n[^b]: second footnote\n");
    }

    #[test]
    fn definition_list_with_multiple_terms() {
        let r = render_all();
        let _ = r("term1\n: definition one\n\nterm2\n: definition two\n: another defn for term2\n");
    }

    #[test]
    fn unicode_content_renders() {
        // Wide range of unicode: latin, CJK, RTL, emoji, combining marks.
        let _ = render_default(
            "héllo wörld — 日本語 — مرحبا — 🌍 — café (with combining é: cafe\u{0301})",
        );
    }

    #[test]
    fn very_long_paragraph_renders() {
        let big = "word ".repeat(500);
        let _ = render_default(&big);
    }

    #[test]
    fn deeply_nested_list_renders() {
        let mut src = String::new();
        for i in 0..8 {
            src.push_str(&"  ".repeat(i));
            src.push_str("- level\n");
        }
        let _ = render_default(&src);
    }

    #[test]
    fn empty_code_block_renders() {
        let _ = render_default("```\n```");
    }

    #[test]
    fn unclosed_code_block_renders() {
        // Half-open fence — common during streaming.
        let _ = render_default("```rust\nfn x() {");
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn provisional_dot_block_strips_diagram_info() {
        // While streaming, an unclosed ```dot fence is provisional.
        // Rendering the diagram engine on partial input fails repeatedly
        // until the closing fence arrives — so the renderer should
        // downgrade the CodeBlock's info to None and fall through to the
        // plain-text branch instead of dispatching to graphviz.
        let renderer = Renderer::default().with_options(IcedOptions {
            graphviz: true,
            ..Default::default()
        });
        // Half-open: opening fence + partial graph, no closing ```.
        let _ = renderer.render("```dot\ndigraph G { a -> b");
        // Cache must remain empty — a successful (or attempted) graphviz
        // dispatch would have inserted an entry keyed on the partial
        // source.
        assert!(
            renderer.svg_cache.borrow().is_empty(),
            "graphviz must not be invoked on a provisional code block",
        );
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn closed_dot_block_still_dispatches() {
        // Sanity: once the fence closes, the diagram engine runs and
        // populates the cache.
        let renderer = Renderer::default().with_options(IcedOptions {
            graphviz: true,
            ..Default::default()
        });
        let _ = renderer.render("```dot\ndigraph G { a -> b }\n```\n");
        assert!(
            !renderer.svg_cache.borrow().is_empty(),
            "graphviz must dispatch once the fence is closed",
        );
    }

    #[test]
    fn malformed_table_does_not_panic() {
        let r = render_all();
        // Mismatched column counts.
        let _ = r("| a | b | c |\n|---|---|\n| 1 |\n| 2 | 3 | 4 | 5 |");
    }

    // ─── Prefix-resilience: streaming fuzz ───────────────────────────

    /// Comprehensive markdown source touching most variants. Used by
    /// the prefix test so every byte-prefix gets exercised.
    const STREAMING_CORPUS: &str = "\
# Heading 1
## Heading 2
### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6

A paragraph with **bold**, *italic*, ~~strike~~, ==highlight==, `code`,
H~2~O, E=mc^2^, a [link](https://example.com), an autolink
<https://example.org>, and an ![image](pic.png).

> A blockquote with **bold**.
> > Nested blockquote.

> [!WARNING]
> Alert body.

- one
- two
  - nested
- [x] done
- [ ] todo

1. first
2. second
3. third

```rust
fn main() {
    println!(\"hi\");
}
```

| col a | col b | col c |
|:------|:-----:|------:|
| 1     | 2     | 3     |
| four  | five  | six   |

term
: definition

see[^1] for more.

[^1]: footnote body

$inline math: a+b$

$$
display math: x^2+y^2=z^2
$$

<div>raw html</div>

---

Final line.
";

    #[test]
    fn streaming_prefix_resilience() {
        // Every byte prefix of the corpus must render without panic.
        // Skip non-char-boundary indices to avoid str slice errors.
        let renderer = Renderer::default().with_options(IcedOptions::all());
        let s = STREAMING_CORPUS;
        for n in 0..=s.len() {
            if !s.is_char_boundary(n) {
                continue;
            }
            let _ = renderer.render(&s[..n]);
        }
    }

    #[test]
    fn streaming_prefix_with_extension_does_not_panic() {
        // Same as above but exercising the Custom dispatch path.
        struct NoopCustom;
        impl<E> RenderIcedCustom<E> for NoopCustom {
            fn render_custom(&self, _: &E, _: &[Node<E>]) -> Element<'static, Url> {
                Column::new().into()
            }
        }
        let renderer = Renderer::new(NoopCustom).with_options(IcedOptions::all());
        let s = STREAMING_CORPUS;
        for n in 0..=s.len() {
            if !s.is_char_boundary(n) {
                continue;
            }
            let _ = renderer.render(&s[..n]);
        }
    }
}
