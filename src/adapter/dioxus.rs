//! Dioxus 0.7 adapter — turn parsed markdown into a `dioxus::Element` for
//! SSG / SSR composition.
//!
//! ```ignore
//! use dioxus::prelude::*;
//! use markdawn::adapter::dioxus::{DioxusOptions, Markdown};
//!
//! #[component]
//! fn BlogPost() -> Element {
//!     let source = include_str!("posts/intro.md");
//!     rsx! {
//!         article { class: "post",
//!             Markdown { source: source.to_string() }
//!         }
//!     }
//! }
//! ```
//!
//! For one-shot rendering without a component:
//!
//! ```ignore
//! let element = render_markdown(source, DioxusOptions::default());
//! let html = dioxus_ssr::render_element(element);
//! ```
//!
//! For pikchr / syntax-highlight / custom-extension support, build a
//! [`Renderer`]:
//!
//! ```ignore
//! let renderer = Renderer::default()
//!     .with_options(DioxusOptions::all())
//!     .with_highlighter(my_highlighter);
//! let html = dioxus_ssr::render_element(renderer.render(source));
//! ```

use ::dioxus::prelude::*;

use crate::node::{AlertKind, Node, TableAlignment};
use crate::parser::Parser;

/// Parser + renderer configuration. Mirrors the parser feature flags.
/// `Clone + PartialEq + Default` so it can be used as a `Markdown`
/// component prop.
// [spec:mw:def:dioxus.surface.options+1]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DioxusOptions {
    pub gfm: bool,
    pub math: bool,
    pub alerts: bool,
    pub highlights: bool,
    pub definition_lists: bool,
    /// When the `pikchr` feature is enabled, fenced code blocks tagged
    /// `pikchr` are rendered as inline SVG via `npikchr`. No-op otherwise.
    #[cfg(feature = "pikchr")]
    pub pikchr: bool,
    /// When the `graphviz` feature is enabled, fenced code blocks tagged
    /// `dot` or `graphviz` are rendered as inline SVG via `graphviz-sys`.
    /// Requires the Graphviz C libraries at build time. No-op otherwise.
    #[cfg(feature = "graphviz")]
    pub graphviz: bool,
    /// When the `typst` feature is enabled, fenced code blocks tagged
    /// `typst` are compiled to inline SVG. No-op otherwise.
    #[cfg(feature = "typst")]
    pub typst: bool,
}

impl DioxusOptions {
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

/// Plug-in for rendering `Node::Custom` payloads as Dioxus elements.
pub trait RenderDioxusCustom<Ext> {
    fn render_custom(&self, payload: &Ext, children: &[Node<Ext>]) -> Element;
}

impl<Ext> RenderDioxusCustom<Ext> for () {
    fn render_custom(&self, _payload: &Ext, _children: &[Node<Ext>]) -> Element {
        rsx! {}
    }
}

/// Callback that returns a per-kind icon to render inside the alert
/// title. Supplied by the caller so markdawn stays agnostic of any
/// particular icon library.
pub type AlertIconFn = Box<dyn Fn(AlertKind) -> Element>;

/// Stateful renderer that bundles parser options, an HTML tag filter, an
/// optional syntax highlighter, and an optional custom-extension renderer.
/// Reusable across many `render` calls.
pub struct Renderer<R = ()> {
    options: DioxusOptions,
    custom: R,
    html_filter: crate::HtmlFilter,
    alert_icons: Option<AlertIconFn>,
    #[cfg(feature = "highlight")]
    highlighter: core::cell::RefCell<Option<Box<dyn crate::highlight::SyntaxHighlighter>>>,
    code_block_rewriters: Vec<Box<dyn crate::ext::CodeBlockRewriter>>,
}

impl Default for Renderer<()> {
    fn default() -> Self {
        Self::new(())
    }
}

impl<R> Renderer<R> {
    pub fn new(custom: R) -> Self {
        Self {
            options: DioxusOptions::default(),
            custom,
            html_filter: crate::HtmlFilter::safe(),
            alert_icons: None,
            #[cfg(feature = "highlight")]
            highlighter: core::cell::RefCell::new(None),
            code_block_rewriters: Vec::new(),
        }
    }

    pub fn with_options(mut self, options: DioxusOptions) -> Self {
        self.options = options;
        self
    }

    /// Override the HTML tag filter (default [`crate::HtmlFilter::safe`]).
    /// Pass [`crate::HtmlFilter::unrestricted`] to opt in to raw HTML.
    pub fn with_html_filter(mut self, filter: crate::HtmlFilter) -> Self {
        self.html_filter = filter;
        self
    }

    /// Install a per-kind icon renderer for GitHub-style alerts.
    /// The callback runs once per rendered alert to produce the leading
    /// icon inside the title row. Markdawn stays icon-library-agnostic —
    /// the caller picks the glyphs.
    pub fn with_alert_icons<F>(mut self, render: F) -> Self
    where
        F: Fn(AlertKind) -> Element + 'static,
    {
        self.alert_icons = Some(Box::new(render));
        self
    }

    /// Install a syntax highlighter. Used to render fenced code blocks
    /// with language-aware coloring. Highlighter failures fall back to
    /// plain `<pre><code>`.
    #[cfg(feature = "highlight")]
    pub fn with_highlighter<H>(self, highlighter: H) -> Self
    where
        H: crate::highlight::SyntaxHighlighter + 'static,
    {
        *self.highlighter.borrow_mut() = Some(Box::new(highlighter));
        self
    }

    /// Install a code-block rewriter. Rewriters are invoked inside
    /// [`render_code_block`](Self::render_code_block) before pikchr /
    /// highlighter / plain dispatch; the first rewriter that returns
    /// `Some` replaces the `info` + `literal` fed to the rest of the
    /// code-block pipeline. Call repeatedly to register multiple
    /// rewriters — they're tried in registration order.
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
    R: RenderDioxusCustom<()>,
{
    /// Parse `source` and render the resulting block list.
    // [spec:mw:sem:dioxus.render.totality]
    pub fn render(&self, source: &str) -> Element {
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
            Err(_) => return rsx! {},
        };
        let nodes: Vec<Node<()>> = blocks.into_iter().map(|b| b.node).collect();
        rsx! { for node in nodes.iter() { {self.render_node(node)} } }
    }
}

impl<R> Renderer<R> {
    /// Render a single `Node` (block or inline) as a Dioxus `Element`.
    // [spec:mw:req:dioxus.structure.vdom-agreement+1]
    pub fn render_node<Ext>(&self, node: &Node<Ext>) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        match node {
            Node::Document { children, .. } => self.render_children(children),
            Node::DefinitionList { children, .. } => rsx! {
                dl { {self.render_children(children)} }
            },
            Node::FootnoteSection { children, .. } => self.render_footnote_section(children),
            Node::Paragraph { children, .. } => rsx! { p { {self.render_children(children)} } },
            Node::Heading {
                level, children, ..
            } => self.render_heading(*level, children),
            Node::BlockQuote { children, .. } => rsx! {
                blockquote { {self.render_children(children)} }
            },
            Node::Alert {
                kind,
                title,
                children,
                ..
            } => self.render_alert(*kind, title.as_deref(), children),
            Node::List {
                ordered,
                start,
                tight,
                children,
                ..
            } => self.render_list(*ordered, *start, tight.unwrap_or(true), children),
            Node::Item { children, .. } => self.render_item(children, false, false),
            Node::TaskItem {
                checked, children, ..
            } => self.render_item(children, true, *checked),
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => self.render_table(alignments, header, rows),
            Node::TableRow { .. } => self.render_table_row(node, &[]),
            Node::TableCell {
                children,
                header,
                alignment,
                ..
            } => self.render_table_cell(*header, *alignment, children),
            Node::CodeBlock { info, literal, .. } => {
                self.render_code_block(info.as_deref(), literal)
            }
            Node::ThematicBreak { .. } => rsx! { hr {} },
            // [spec:mw:req:dioxus.html.host-element+1]
            Node::Html { html, .. } => {
                let safe = self.html_filter.filter(html);
                rsx! { div { dangerous_inner_html: safe } }
            }
            Node::Text { text, .. } => rsx! { "{text}" },
            Node::Emphasis { children, .. } => rsx! { em { {self.render_children(children)} } },
            Node::Strong { children, .. } => rsx! { strong { {self.render_children(children)} } },
            Node::Strikethrough { children, .. } => {
                rsx! { del { {self.render_children(children)} } }
            }
            Node::Highlight { children, .. } => rsx! { mark { {self.render_children(children)} } },
            Node::Subscript { children, .. } => rsx! { sub { {self.render_children(children)} } },
            Node::Superscript { children, .. } => rsx! { sup { {self.render_children(children)} } },
            Node::Link {
                destination,
                title,
                children,
                ..
            } => self.render_link(destination, title.as_deref(), children),
            Node::Image {
                destination,
                title,
                alt,
                ..
            } => render_image(destination, title.as_deref(), alt),
            Node::CodeSpan { code, .. } => rsx! { code { "{code}" } },
            Node::MathInline { math, .. } => render_math(math, false),
            Node::MathBlock { math, .. } => render_math(math, true),
            Node::SoftBreak { .. } => rsx! { " " },
            Node::HardBreak { .. } => rsx! { br {} },
            Node::FootnoteRef {
                label,
                number,
                ref_num,
                ..
            } => render_footnote_ref(label, *number, *ref_num),
            Node::FootnoteDefinition {
                label,
                number,
                backref_count,
                children,
                ..
            } => self.render_footnote_definition(label, *number, *backref_count, children),
            Node::DefinitionTerm { children, .. } => rsx! {
                dt { {self.render_children(children)} }
            },
            Node::DefinitionDescription { children, .. } => rsx! {
                dd { {self.render_children(children)} }
            },
            Node::Custom {
                payload, children, ..
            } => self.custom.render_custom(payload, children),
        }
    }

    fn render_children<Ext>(&self, children: &[Node<Ext>]) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        rsx! { for child in children.iter() { {self.render_node(child)} } }
    }

    fn render_heading<Ext>(&self, level: u8, children: &[Node<Ext>]) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        match level {
            1 => rsx! { h1 { {self.render_children(children)} } },
            2 => rsx! { h2 { {self.render_children(children)} } },
            3 => rsx! { h3 { {self.render_children(children)} } },
            4 => rsx! { h4 { {self.render_children(children)} } },
            5 => rsx! { h5 { {self.render_children(children)} } },
            _ => rsx! { h6 { {self.render_children(children)} } },
        }
    }

    // [spec:mw:req:core.node.alert-title-fallback+1]
    fn render_alert<Ext>(
        &self,
        kind: AlertKind,
        custom_title: Option<&str>,
        children: &[Node<Ext>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let (slug, default_title) = match kind {
            AlertKind::Note => ("note", "Note"),
            AlertKind::Tip => ("tip", "Tip"),
            AlertKind::Important => ("important", "Important"),
            AlertKind::Warning => ("warning", "Warning"),
            AlertKind::Caution => ("caution", "Caution"),
        };
        let title = custom_title.unwrap_or(default_title).to_string();
        let icon = self.alert_icons.as_ref().map(|f| f(kind));
        rsx! {
            div { class: "markdown-alert markdown-alert-{slug}",
                p { class: "markdown-alert-title",
                    if let Some(icon) = icon {
                        span { class: "markdown-alert-icon", {icon} }
                    }
                    span { class: "markdown-alert-label", "{title}" }
                }
                {self.render_children(children)}
            }
        }
    }

    fn render_list<Ext>(
        &self,
        ordered: bool,
        start: Option<u64>,
        tight: bool,
        children: &[Node<Ext>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let items = rsx! {
            for child in children.iter() { {self.render_list_child(child, tight)} }
        };
        if ordered {
            let start = start.unwrap_or(1);
            rsx! { ol { start: start as i64, {items} } }
        } else {
            rsx! { ul { {items} } }
        }
    }

    fn render_list_child<Ext>(&self, node: &Node<Ext>, tight: bool) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        match node {
            Node::Item { children, .. } => {
                self.render_item_with_tight(children, false, false, tight)
            }
            Node::TaskItem {
                checked, children, ..
            } => self.render_item_with_tight(children, true, *checked, tight),
            other => self.render_node(other),
        }
    }

    fn render_item<Ext>(&self, children: &[Node<Ext>], task: bool, checked: bool) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        // Standalone item (outside a List context). Default to tight rendering
        // since CommonMark single-item lists are tight.
        self.render_item_with_tight(children, task, checked, true)
    }

    fn render_item_with_tight<Ext>(
        &self,
        children: &[Node<Ext>],
        task: bool,
        checked: bool,
        tight: bool,
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let body = rsx! {
            for (i, child) in children.iter().enumerate() {
                {self.render_item_child(child, i, children.len(), tight)}
            }
        };
        if task {
            rsx! {
                li {
                    input { r#type: "checkbox", disabled: true, checked: checked }
                    " "
                    {body}
                }
            }
        } else {
            rsx! { li { {body} } }
        }
    }

    fn render_item_child<Ext>(
        &self,
        child: &Node<Ext>,
        idx: usize,
        total: usize,
        tight: bool,
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        if tight
            && let Node::Paragraph {
                children: inlines, ..
            } = child
        {
            // Tight list: skip the <p> wrapper, optionally insert a soft
            // break between consecutive paragraphs.
            let leading_break = idx > 0;
            return rsx! {
                if leading_break { "\n" }
                {self.render_children(inlines)}
            };
        }
        let _ = total;
        self.render_node(child)
    }

    fn render_footnote_section<Ext>(&self, children: &[Node<Ext>]) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        if children.is_empty() {
            return rsx! {};
        }
        rsx! {
            section { class: "footnotes", "data-footnotes": "true",
                ol { {self.render_children(children)} }
            }
        }
    }

    fn render_footnote_definition<Ext>(
        &self,
        label: &str,
        number: u32,
        backref_count: u32,
        children: &[Node<Ext>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let id = format!("fn-{label}");
        // Append backref(s) to the last paragraph, or as a tail if the last
        // child isn't a paragraph (matches the HTML adapter behavior).
        if children.is_empty() {
            return rsx! {
                li { id: id, {render_footnote_backrefs(label, number, backref_count)} }
            };
        }
        let last_idx = children.len() - 1;
        rsx! {
            li { id: id,
                for (idx, child) in children.iter().enumerate() {
                    if idx == last_idx {
                        if let Node::Paragraph { children: inlines, .. } = child {
                            p {
                                {self.render_children(inlines)}
                                " "
                                {render_footnote_backrefs(label, number, backref_count)}
                            }
                        } else {
                            {self.render_node(child)}
                            {render_footnote_backrefs(label, number, backref_count)}
                        }
                    } else {
                        {self.render_node(child)}
                    }
                }
            }
        }
    }

    fn render_table<Ext>(
        &self,
        alignments: &[Option<TableAlignment>],
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        rsx! {
            table {
                thead {
                    for row in header.iter() { {self.render_table_row(row, alignments)} }
                }
                if !rows.is_empty() {
                    tbody {
                        for row in rows.iter() { {self.render_table_row(row, alignments)} }
                    }
                }
            }
        }
    }

    fn render_table_row<Ext>(
        &self,
        node: &Node<Ext>,
        alignments: &[Option<TableAlignment>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let Node::TableRow { cells, .. } = node else {
            return rsx! {};
        };
        rsx! {
            tr {
                for (index, cell) in cells.iter().enumerate() {
                    {self.render_row_cell(cell, alignments.get(index).copied().flatten())}
                }
            }
        }
    }

    fn render_row_cell<Ext>(&self, cell: &Node<Ext>, column: Option<TableAlignment>) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        match cell {
            Node::TableCell {
                children,
                header,
                alignment,
                ..
            } => self.render_table_cell(*header, alignment.or(column), children),
            _ => rsx! {},
        }
    }

    fn render_table_cell<Ext>(
        &self,
        header: bool,
        alignment: Option<TableAlignment>,
        children: &[Node<Ext>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let style = match alignment {
            Some(TableAlignment::Left) => "text-align: left",
            Some(TableAlignment::Center) => "text-align: center",
            Some(TableAlignment::Right) => "text-align: right",
            None => "",
        };
        if header {
            rsx! { th { style: style, {self.render_children(children)} } }
        } else {
            rsx! { td { style: style, {self.render_children(children)} } }
        }
    }

    fn render_link<Ext>(
        &self,
        destination: &str,
        title: Option<&str>,
        children: &[Node<Ext>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let title = title.unwrap_or_default();
        let href = if crate::url_safety::is_safe_url(destination) {
            destination.to_string()
        } else {
            String::new()
        };
        rsx! {
            a {
                href: href,
                title: title.to_string(),
                {self.render_children(children)}
            }
        }
    }

    // [spec:mw:req:core.rewriter.dispatch-order]
    // [spec:mw:req:core.rewriter.render-time-only]
    fn render_code_block(&self, info: Option<&str>, literal: &str) -> Element {
        // 0. user-registered rewriters. The first one that returns Some
        //    substitutes info + literal for the rest of the pipeline; a
        //    rewriter producing info = Some("pikchr") therefore routes to
        //    the pikchr branch below, producing info = Some("rust") routes
        //    through the highlighter, etc.
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

        // Helper: the "language" is the first whitespace-delimited word of
        // the info string. Info strings may carry trailing metadata
        // (attributes, filenames, etc.) consumed by rewriters.
        let info_lang = info.map(|s| s.split_whitespace().next().unwrap_or(""));

        // 0.5. ```ansi / ```terminal — render escape sequences as styled
        //      HTML, wrapped in the CSS-only two-pane toggle so readers
        //      can flip to the annotated-source view.
        if info_lang == Some("ansi") {
            return render_ansi_block(literal);
        }
        if info_lang == Some("terminal") {
            return render_terminal_block(info, literal);
        }

        // 1. pikchr (feature-gated)
        #[cfg(feature = "pikchr")]
        if self.options.pikchr
            && info_lang == Some("pikchr")
            && let Ok(svg) = crate::pikchr_render::render(literal)
            && svg.len() <= crate::svg_sanitize::MAX_SVG_BYTES
        {
            let sanitized = crate::svg_sanitize::sanitize_svg(&svg);
            let final_svg = crate::svg_text_to_paths::text_to_paths(&sanitized);
            return rsx! {
                div { class: "pikchr", dangerous_inner_html: final_svg }
            };
        }

        // 1b. graphviz (feature-gated)
        #[cfg(feature = "graphviz")]
        if self.options.graphviz
            && matches!(info_lang, Some("dot") | Some("graphviz"))
            && let Ok(svg) = markdawn_graphviz::render_dot_to_svg(literal)
            && svg.len() <= crate::svg_sanitize::MAX_SVG_BYTES
        {
            let sanitized = crate::svg_sanitize::sanitize_svg(&svg);
            let final_svg = crate::svg_text_to_paths::text_to_paths(&sanitized);
            return rsx! {
                div { class: "graphviz", dangerous_inner_html: final_svg }
            };
        }

        // 1c. typst (feature-gated)
        #[cfg(feature = "typst")]
        if self.options.typst
            && info_lang == Some("typst")
            && let Some(svg) = crate::render_typst_to_svg(literal)
        {
            return rsx! {
                div { class: "typst", dangerous_inner_html: svg }
            };
        }

        // 2. syntax highlighter (feature-gated)
        #[cfg(feature = "highlight")]
        if let Some(lang) = info_lang.filter(|s| !s.is_empty()) {
            let mut borrow = self.highlighter.borrow_mut();
            if let Some(hl) = borrow.as_mut() {
                // [spec:mw:req:highlighting.error-fallback]
                if let Ok(spans) = hl.highlight(lang, literal) {
                    let html = crate::highlight::spans_to_html(literal, spans);
                    let class = format!("language-{lang}");
                    return rsx! {
                        pre {
                            code { class: class, dangerous_inner_html: html }
                        }
                    };
                }
            }
        }

        // 3. plain fallback
        let class = match info_lang {
            Some(lang) if !lang.is_empty() => format!("language-{lang}"),
            _ => String::new(),
        };
        rsx! { pre { code { class: class, "{literal}" } } }
    }
}

/// Drop-in component for `rsx!` composition.
// [spec:mw:req:dioxus.props.inert]
#[component]
pub fn Markdown(source: String, #[props(default)] options: DioxusOptions) -> Element {
    render_markdown(&source, options)
}

/// Parse `source` and render the resulting block list as a single `Element`.
/// For pikchr / syntax-highlighter / custom-extension support, build a
/// [`Renderer`] instead.
// [spec:mw:def:dioxus.surface.entry-points]
pub fn render_markdown(source: &str, options: DioxusOptions) -> Element {
    Renderer::default().with_options(options).render(source)
}

/// Render a single pre-parsed `Node` with a custom-extension renderer.
pub fn render_node<Ext, R>(node: &Node<Ext>, custom: &R) -> Element
where
    R: RenderDioxusCustom<Ext>,
{
    // Build a temporary renderer that borrows `custom`. Cheap because R
    // typically has no state.
    struct Borrow<'a, R>(&'a R);
    impl<R, Ext> RenderDioxusCustom<Ext> for Borrow<'_, R>
    where
        R: RenderDioxusCustom<Ext>,
    {
        fn render_custom(&self, payload: &Ext, children: &[Node<Ext>]) -> Element {
            self.0.render_custom(payload, children)
        }
    }
    Renderer::new(Borrow(custom)).render_node(node)
}

fn render_image(destination: &str, title: Option<&str>, alt: &str) -> Element {
    let title = title.unwrap_or_default();
    let src = if crate::url_safety::is_safe_image_url(destination) {
        destination.to_string()
    } else {
        String::new()
    };
    rsx! {
        img {
            src: src,
            alt: alt.to_string(),
            title: title.to_string(),
        }
    }
}

fn render_math(math: &str, display: bool) -> Element {
    let svg = mathml_core::latex_to_svg(math, if display { 22.0 } else { 16.0 });
    match svg {
        Ok(svg) if display => rsx! {
            div { class: "math math-display", dangerous_inner_html: svg }
        },
        Ok(svg) => rsx! {
            span { class: "math math-inline", dangerous_inner_html: svg }
        },
        Err(_) if display => rsx! {
            pre {
                code { class: "language-math math-display math-error", "{math}" }
            }
        },
        Err(_) => rsx! {
            code { class: "language-math math-inline math-error", "{math}" }
        },
    }
}

/// Monotonic id for the CSS-only two-pane toggles, whose `<label for>`
/// has to address its own `<input id>`.
fn next_pane_id() -> usize {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[cfg(feature = "ansi")]
fn render_ansi_block(literal: &str) -> Element {
    let rendered = crate::ansi_to_html::ansi_to_html(literal);
    let source = crate::ansi_to_html::ansi_to_annotated_source(literal);
    let id_str = format!("ansi-src-{}", next_pane_id());
    rsx! {
        div { class: "ansi-block",
            input {
                r#type: "checkbox",
                id: "{id_str}",
                class: "ansi-toggle-input",
                aria_hidden: "true",
            }
            label {
                r#for: "{id_str}",
                class: "ansi-toggle",
                span { class: "ansi-toggle-show", "show source" }
                span { class: "ansi-toggle-hide", "show rendered" }
            }
            pre { class: "ansi-rendered",
                code { class: "language-ansi", dangerous_inner_html: rendered }
            }
            pre { class: "ansi-source",
                code { class: "language-ansi-source", dangerous_inner_html: source }
            }
        }
    }
}

#[cfg(not(feature = "ansi"))]
fn render_ansi_block(literal: &str) -> Element {
    rsx! {
        pre {
            code { class: "language-ansi", "{literal}" }
        }
    }
}

#[cfg(feature = "terminal")]
fn render_terminal_block(info: Option<&str>, literal: &str) -> Element {
    let (cols, rows) = crate::adapter::vdom::parse_terminal_geometry(info);
    let rendered = crate::terminal::ansi_snapshot_to_html(literal, cols, rows);
    let source = crate::ansi_to_html::ansi_to_annotated_source(literal);
    let id_str = format!("terminal-src-{}", next_pane_id());
    rsx! {
        div { class: "terminal-block",
            input {
                r#type: "checkbox",
                id: "{id_str}",
                class: "terminal-toggle-input",
                aria_hidden: "true",
            }
            label {
                r#for: "{id_str}",
                class: "terminal-toggle",
                span { class: "terminal-toggle-show", "show source" }
                span { class: "terminal-toggle-hide", "show rendered" }
            }
            pre { class: "terminal-rendered",
                code { class: "language-terminal", dangerous_inner_html: rendered }
            }
            pre { class: "terminal-source",
                code { class: "language-terminal-source", dangerous_inner_html: source }
            }
        }
    }
}

#[cfg(not(feature = "terminal"))]
fn render_terminal_block(_info: Option<&str>, literal: &str) -> Element {
    rsx! {
        pre {
            code { class: "language-terminal", "{literal}" }
        }
    }
}

fn render_footnote_ref(label: &str, number: u32, ref_num: u32) -> Element {
    let suffix = if ref_num > 1 {
        format!("-{ref_num}")
    } else {
        String::new()
    };
    let href = format!("#fn-{label}");
    let id = format!("fnref-{label}{suffix}");
    rsx! {
        sup { class: "footnote-ref",
            a { href: href, id: id, "data-footnote-ref": "true", "{number}" }
        }
    }
}

fn render_footnote_backrefs(label: &str, number: u32, count: u32) -> Element {
    let count = count.max(1);
    rsx! {
        for i in 1..=count {
            {
                let href = if i > 1 {
                    format!("#fnref-{label}-{i}")
                } else {
                    format!("#fnref-{label}")
                };
                let idx = if i > 1 {
                    format!("{number}-{i}")
                } else {
                    number.to_string()
                };
                let aria = if i > 1 {
                    format!("Back to reference {number}-{i}")
                } else {
                    format!("Back to reference {number}")
                };
                rsx! {
                    if i > 1 { " " }
                    a {
                        href: href,
                        class: "footnote-backref",
                        "data-footnote-backref": "true",
                        "data-footnote-backref-idx": idx,
                        "aria-label": aria,
                        "↩"
                        if i > 1 {
                            sup { class: "footnote-ref", "{i}" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "dioxus_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dioxus_surface_tests.rs"]
mod surface_tests;

#[cfg(test)]
#[path = "dioxus_vdom_parity_tests.rs"]
mod vdom_parity_tests;
