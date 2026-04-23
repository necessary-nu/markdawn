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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DioxusOptions {
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
    /// `dot` or `graphviz` are rendered as inline SVG via `graphviz-sys`.
    /// Requires the Graphviz C libraries at build time. No-op otherwise.
    #[cfg(feature = "graphviz")]
    pub graphviz: bool,
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
    highlighter: core::cell::RefCell<Option<Box<dyn crate::highlight::SpanHighlighter>>>,
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
        H: crate::highlight::SpanHighlighter + 'static,
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
    pub fn render_node<Ext>(&self, node: &Node<Ext>) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        match node {
            Node::Document { children, .. } | Node::DefinitionList { children, .. } => {
                self.render_children(children)
            }
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
            Node::TableRow { cells, .. } => rsx! { tr { {self.render_children(cells)} } },
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
        if tight {
            if let Node::Paragraph {
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
        _alignments: &[Option<TableAlignment>],
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
    ) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        rsx! {
            table {
                if !header.is_empty() {
                    thead { {self.render_children(header)} }
                }
                tbody { {self.render_children(rows)} }
            }
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

        // 0.5. ```ansi — render escape sequences as styled HTML via the
        //      crate-level `ansi_to_html` helper. Dispatches to the
        //      CSS-only two-pane wrapper so readers can flip to the
        //      annotated-source view.
        #[cfg(feature = "ansi")]
        if info_lang == Some("ansi") {
            let rendered = crate::ansi_to_html::ansi_to_html(literal);
            let source = crate::ansi_to_html::ansi_to_annotated_source(literal);
            let id = {
                use core::sync::atomic::{AtomicUsize, Ordering};
                static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            };
            let id_str = format!("ansi-src-{id}");
            return rsx! {
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
            };
        }

        // 1. pikchr (feature-gated)
        #[cfg(feature = "pikchr")]
        if self.options.pikchr && info_lang == Some("pikchr") {
            if let Ok(svg) = crate::pikchr_render::render(literal) {
                if svg.len() <= crate::svg_sanitize::MAX_SVG_BYTES {
                    let sanitized = crate::svg_sanitize::sanitize_svg(&svg);
                    let final_svg = crate::svg_text_to_paths::text_to_paths(&sanitized);
                    return rsx! {
                        div { class: "pikchr", dangerous_inner_html: final_svg }
                    };
                }
            }
        }

        // 1b. graphviz (feature-gated)
        #[cfg(feature = "graphviz")]
        if self.options.graphviz && matches!(info_lang, Some("dot") | Some("graphviz")) {
            if let Ok(svg) = markdawn_graphviz::render_dot_to_svg(literal) {
                if svg.len() <= crate::svg_sanitize::MAX_SVG_BYTES {
                    let sanitized = crate::svg_sanitize::sanitize_svg(&svg);
                    let final_svg = crate::svg_text_to_paths::text_to_paths(&sanitized);
                    return rsx! {
                        div { class: "graphviz", dangerous_inner_html: final_svg }
                    };
                }
            }
        }

        // 2. syntax highlighter (feature-gated)
        #[cfg(feature = "highlight")]
        if let Some(lang) = info_lang.filter(|s| !s.is_empty()) {
            let mut borrow = self.highlighter.borrow_mut();
            if let Some(hl) = borrow.as_mut() {
                if let Some(spans) = hl.spans(lang, literal) {
                    let html = arborium::advanced::spans_to_html(
                        literal,
                        spans,
                        &arborium::advanced::HtmlFormat::default(),
                    );
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
#[component]
pub fn Markdown(source: String, #[props(default)] options: DioxusOptions) -> Element {
    render_markdown(&source, options)
}

/// Parse `source` and render the resulting block list as a single `Element`.
/// For pikchr / syntax-highlighter / custom-extension support, build a
/// [`Renderer`] instead.
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
    let src = if crate::url_safety::is_safe_url(destination) {
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
            pre { class: "math math-display math-error", "{math}" }
        },
        Err(_) => rsx! {
            code { class: "math math-inline math-error", "{math}" }
        },
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
mod tests {
    use super::*;

    fn ssr(element: Element) -> String {
        dioxus_ssr::render_element(element)
    }

    #[test]
    fn paragraph_renders_p_tag() {
        let html = ssr(render_markdown("hello world", DioxusOptions::default()));
        assert_eq!(html, "<p>hello world</p>");
    }

    #[test]
    fn heading_renders_correct_tag() {
        let html = ssr(render_markdown("## Subheading", DioxusOptions::default()));
        assert_eq!(html, "<h2>Subheading</h2>");
    }

    #[test]
    fn nested_blockquote_with_heading() {
        let html = ssr(render_markdown("> # title", DioxusOptions::default()));
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("<h1>title</h1>"));
        assert!(html.contains("</blockquote>"));
    }

    #[test]
    fn link_has_href_attribute() {
        let html = ssr(render_markdown(
            "[click](https://example.com)",
            DioxusOptions::default(),
        ));
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains(">click</a>"));
    }

    // adv[verify markdown.url-allowlist]
    #[test]
    fn javascript_link_href_is_stripped() {
        let html = ssr(render_markdown(
            "[x](javascript:alert(1))",
            DioxusOptions::default(),
        ));
        assert!(!html.contains("javascript:"), "got: {html}");
        assert!(html.contains("href=\"\""), "got: {html}");
    }

    #[test]
    fn javascript_image_src_is_stripped() {
        let html = ssr(render_markdown(
            "![x](javascript:alert(1))",
            DioxusOptions::default(),
        ));
        assert!(!html.contains("javascript:"), "got: {html}");
    }

    #[test]
    fn data_url_in_link_is_stripped() {
        // Base64-encoded `<script>alert(1)</script>` without parens so
        // markdown parses the URL cleanly; sanitiser should still reject
        // the `data:` scheme.
        let html = ssr(render_markdown(
            "[x](data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==)",
            DioxusOptions::default(),
        ));
        assert!(!html.contains("data:"), "got: {html}");
    }

    #[test]
    fn vbscript_url_in_link_is_stripped() {
        let html = ssr(render_markdown(
            "[x](vbscript:msgbox)",
            DioxusOptions::default(),
        ));
        assert!(!html.contains("vbscript:"), "got: {html}");
    }

    #[test]
    fn code_block_uses_language_class() {
        let html = ssr(render_markdown(
            "```rust\nlet x = 1;\n```",
            DioxusOptions::default(),
        ));
        assert!(html.contains("class=\"language-rust\""));
        assert!(html.contains("let x = 1;"));
    }

    #[test]
    fn alert_renders_github_class_structure() {
        let html = ssr(render_markdown(
            "> [!WARNING]\n> careful",
            DioxusOptions {
                alerts: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("markdown-alert"));
        assert!(html.contains("markdown-alert-warning"));
        assert!(html.contains(">Warning<"));
        assert!(html.contains("careful"));
    }

    #[test]
    fn alert_custom_title_overrides_default_label() {
        let html = ssr(render_markdown(
            "> [!NOTE] My custom title\n> body paragraph",
            DioxusOptions {
                alerts: true,
                ..Default::default()
            },
        ));
        assert!(
            html.contains(">My custom title<"),
            "expected custom title; got: {html}"
        );
        assert!(
            !html.contains(">Note<"),
            "default label should be replaced; got: {html}"
        );
        assert!(html.contains("body paragraph"));
    }

    #[test]
    fn alert_without_title_keeps_default_label() {
        let html = ssr(render_markdown(
            "> [!NOTE]\n> body",
            DioxusOptions {
                alerts: true,
                ..Default::default()
            },
        ));
        assert!(
            html.contains(">Note<"),
            "expected default label; got: {html}"
        );
    }

    #[test]
    fn table_alignment_emits_text_align_style() {
        let src = "| a | b |\n|:--|---:|\n| 1 | 2 |";
        let html = ssr(render_markdown(
            src,
            DioxusOptions {
                gfm: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("style=\"text-align: left\""));
        assert!(html.contains("style=\"text-align: right\""));
    }

    #[test]
    fn footnote_ref_renders_sup_link() {
        let src = "see[^1]\n\n[^1]: a footnote";
        let html = ssr(render_markdown(
            src,
            DioxusOptions {
                gfm: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("class=\"footnote-ref\""));
        assert!(html.contains("href=\"#fn-1\""));
    }

    #[test]
    fn math_block_renders_inline_svg() {
        let html = ssr(render_markdown(
            "$$x^2 + y^2 = z^2$$",
            DioxusOptions {
                math: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("<svg"), "expected SVG, got: {html}");
        assert!(html.contains("class=\"math math-display\""));
    }

    #[test]
    fn math_inline_renders_inline_svg() {
        let html = ssr(render_markdown(
            "the formula $a + b$ is simple",
            DioxusOptions {
                math: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("<svg"), "expected SVG, got: {html}");
        assert!(html.contains("class=\"math math-inline\""));
    }

    #[test]
    fn task_list_item_includes_checkbox() {
        let html = ssr(render_markdown(
            "- [x] done",
            DioxusOptions {
                gfm: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("disabled"));
        assert!(html.contains("checked"));
    }

    #[test]
    fn tight_list_strips_paragraph_wrapper() {
        let html = ssr(render_markdown(
            "- one\n- two\n- three",
            DioxusOptions::default(),
        ));
        // Tight list: <li>one</li>, NOT <li><p>one</p></li>
        assert!(
            !html.contains("<p>one"),
            "tight list shouldn't wrap in <p>: {html}"
        );
        assert!(html.contains("<li>one"), "got: {html}");
    }

    #[test]
    fn loose_list_keeps_paragraph_wrapper() {
        // Blank line between items → loose list
        let html = ssr(render_markdown(
            "- one\n\n- two\n",
            DioxusOptions::default(),
        ));
        assert!(
            html.contains("<p>one</p>"),
            "loose list should wrap in <p>: {html}"
        );
    }

    #[test]
    fn footnote_section_wraps_in_section_and_ol() {
        let src = "see[^1]\n\n[^1]: a footnote";
        let html = ssr(render_markdown(
            src,
            DioxusOptions {
                gfm: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("<section class=\"footnotes\""), "got: {html}");
        assert!(html.contains("data-footnotes"), "got: {html}");
        assert!(html.contains("<ol>"), "got: {html}");
        assert!(html.contains("id=\"fn-1\""), "got: {html}");
    }

    #[test]
    fn footnote_definition_appends_backref() {
        let src = "see[^1]\n\n[^1]: a footnote";
        let html = ssr(render_markdown(
            src,
            DioxusOptions {
                gfm: true,
                ..Default::default()
            },
        ));
        assert!(html.contains("class=\"footnote-backref\""), "got: {html}");
        assert!(html.contains("href=\"#fnref-1\""), "got: {html}");
        assert!(html.contains("↩"), "expected backref arrow: {html}");
    }

    #[test]
    fn html_block_strips_dangerous_tags() {
        let src = "<script>alert('xss')</script>\n\nsafe";
        let html = ssr(render_markdown(
            src,
            DioxusOptions {
                gfm: true,
                ..Default::default()
            },
        ));
        // The tagfilter replaces `<` with `&lt;` for disallowed tags
        assert!(
            !html.contains("<script>") || html.contains("&lt;script"),
            "expected script tag filtered, got: {html}"
        );
    }

    #[test]
    fn unrestricted_filter_passes_script_through() {
        let src = "<script>foo</script>";
        let html = ssr(Renderer::default()
            .with_html_filter(crate::HtmlFilter::unrestricted())
            .render(src));
        // Dioxus's dangerous_inner_html doesn't escape, but the SSR output
        // still HTML-escapes the result of dangerous_inner_html attribute
        // setting. Either way, the tag should NOT be neutered (no &lt;script).
        assert!(!html.contains("&lt;script"), "got: {html}");
    }

    #[cfg(feature = "pikchr")]
    #[test]
    fn pikchr_block_renders_inline_svg() {
        let src = "```pikchr\nbox \"Hello\"\n```";
        let html = ssr(Renderer::default()
            .with_options(DioxusOptions {
                pikchr: true,
                ..Default::default()
            })
            .render(src));
        assert!(html.contains("class=\"pikchr\""), "got: {html}");
        assert!(html.contains("<svg"), "got: {html}");
    }

    #[cfg(feature = "pikchr")]
    #[test]
    fn pikchr_block_falls_back_when_disabled() {
        let src = "```pikchr\nbox \"Hello\"\n```";
        let html = ssr(render_markdown(src, DioxusOptions::default()));
        // pikchr feature off in options → just a code block
        assert!(html.contains("<pre>"));
        assert!(!html.contains("<svg"));
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn graphviz_block_renders_inline_svg() {
        let src = "```dot\ndigraph { A -> B }\n```";
        let html = ssr(Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src));
        assert!(html.contains("class=\"graphviz\""), "got: {html}");
        assert!(html.contains("<svg"), "got: {html}");
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn graphviz_block_falls_back_when_disabled() {
        let src = "```dot\ndigraph { A -> B }\n```";
        let html = ssr(render_markdown(src, DioxusOptions::default()));
        assert!(html.contains("<pre>"));
        assert!(!html.contains("<svg"));
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn graphviz_info_string_alias_also_renders() {
        let src = "```graphviz\ndigraph { A -> B }\n```";
        let html = ssr(Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src));
        assert!(html.contains("class=\"graphviz\""), "got: {html}");
        assert!(html.contains("<svg"), "got: {html}");
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn graphviz_javascript_href_sanitized_in_output_svg() {
        let src = "```dot\ndigraph { A [href=\"javascript:alert(1)\"] }\n```";
        let html = ssr(Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src));
        assert!(html.contains("class=\"graphviz\""), "no graphviz div: {html}");
        assert!(!html.contains("javascript:"), "leaked javascript: {html}");
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn graphviz_url_alias_javascript_sanitized() {
        // `URL=` is an alias for `href=`. Graphviz emits it as an
        // xlink:href in SVG, where the post-render sanitiser catches
        // the unsafe scheme.
        let src = "```dot\ndigraph { A [URL=\"javascript:alert(1)\"]; B }\n```";
        let html = ssr(Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src));
        assert!(!html.contains("javascript:"), "leaked: {html}");
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn graphviz_html_label_img_path_does_not_leak() {
        let src = "```dot\ndigraph { A [label=<<IMG SRC=\"/etc/passwd\"/>>] }\n```";
        let html = ssr(Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src));
        assert!(!html.contains("/etc/passwd"), "leaked: {html}");
    }

    #[cfg(feature = "graphviz")]
    #[test]
    fn graphviz_safe_href_preserved_in_output_svg() {
        let src = "```dot\ndigraph { A [href=\"https://example.com\"] }\n```";
        let html = ssr(Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src));
        assert!(html.contains("https://example.com"), "link stripped: {html}");
    }

    #[cfg(feature = "highlight")]
    #[test]
    fn highlighter_path_invoked_for_known_language() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct StubHl(Rc<Cell<bool>>);
        impl crate::highlight::SpanHighlighter for StubHl {
            fn spans(&mut self, _lang: &str, _src: &str) -> Option<Vec<arborium::advanced::Span>> {
                self.0.set(true);
                None // fall back to plain rendering
            }
        }

        let invoked = Rc::new(Cell::new(false));
        let html = ssr(Renderer::default()
            .with_highlighter(StubHl(invoked.clone()))
            .render("```rust\nfoo\n```"));
        assert!(
            invoked.get(),
            "highlighter should be queried for `rust` block"
        );
        assert!(html.contains("class=\"language-rust\""), "got: {html}");
    }

    #[cfg(feature = "highlight")]
    #[test]
    fn highlighter_skipped_for_unknown_language_with_no_info() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct StubHl(Rc<Cell<u32>>);
        impl crate::highlight::SpanHighlighter for StubHl {
            fn spans(&mut self, _lang: &str, _src: &str) -> Option<Vec<arborium::advanced::Span>> {
                self.0.set(self.0.get() + 1);
                None
            }
        }

        let calls = Rc::new(Cell::new(0));
        let _ = ssr(Renderer::default()
            .with_highlighter(StubHl(calls.clone()))
            .render("```\nno-lang\n```"));
        assert_eq!(calls.get(), 0, "no info string → highlighter not called");
    }

    #[test]
    fn code_block_rewriter_substitutes_info_and_literal() {
        struct StubRewriter;
        impl crate::ext::CodeBlockRewriter for StubRewriter {
            fn rewrite(
                &self,
                info: Option<&str>,
                _literal: &str,
            ) -> Option<crate::ext::CodeBlockRewrite> {
                if info == Some("foo") {
                    Some(crate::ext::CodeBlockRewrite {
                        info: Some("bar".into()),
                        literal: "hello world".into(),
                    })
                } else {
                    None
                }
            }
        }

        let html = ssr(Renderer::default()
            .with_code_block_rewriter(StubRewriter)
            .render("```foo\nignored content\n```"));
        assert!(
            html.contains("class=\"language-bar\""),
            "info should be substituted; got: {html}"
        );
        assert!(
            html.contains("hello world"),
            "literal should be substituted; got: {html}"
        );
        assert!(
            !html.contains("ignored content"),
            "original literal should not appear; got: {html}"
        );
    }

    #[test]
    fn code_block_rewriter_passes_through_non_matching() {
        struct NeverMatches;
        impl crate::ext::CodeBlockRewriter for NeverMatches {
            fn rewrite(
                &self,
                _info: Option<&str>,
                _literal: &str,
            ) -> Option<crate::ext::CodeBlockRewrite> {
                None
            }
        }

        let html = ssr(Renderer::default()
            .with_code_block_rewriter(NeverMatches)
            .render("```text\nfoo\n```"));
        assert!(html.contains("class=\"language-text\""), "got: {html}");
        assert!(html.contains("foo"), "got: {html}");
    }
}
