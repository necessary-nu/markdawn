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

/// Stateful renderer that bundles parser options, an optional syntax
/// highlighter, and an optional custom-extension renderer. Reusable across
/// many `render` calls.
pub struct Renderer<R = ()> {
    options: DioxusOptions,
    custom: R,
    #[cfg(feature = "highlight")]
    highlighter: core::cell::RefCell<Option<Box<dyn crate::highlight::SpanHighlighter>>>,
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
            #[cfg(feature = "highlight")]
            highlighter: core::cell::RefCell::new(None),
        }
    }

    pub fn with_options(mut self, options: DioxusOptions) -> Self {
        self.options = options;
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
            Node::Document { children, .. }
            | Node::FootnoteSection { children, .. }
            | Node::DefinitionList { children, .. } => self.render_children(children),
            Node::Paragraph { children, .. } => rsx! { p { {self.render_children(children)} } },
            Node::Heading {
                level, children, ..
            } => self.render_heading(*level, children),
            Node::BlockQuote { children, .. } => rsx! {
                blockquote { {self.render_children(children)} }
            },
            Node::Alert { kind, children, .. } => self.render_alert(*kind, children),
            Node::List {
                ordered,
                start,
                children,
                ..
            } => self.render_list(*ordered, *start, children),
            Node::Item { children, .. } => rsx! { li { {self.render_children(children)} } },
            Node::TaskItem {
                checked, children, ..
            } => rsx! {
                li {
                    input { r#type: "checkbox", disabled: true, checked: *checked }
                    {self.render_children(children)}
                }
            },
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
            Node::Html { html, .. } => rsx! {
                div { dangerous_inner_html: html.clone() }
            },
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
                label, children, ..
            } => rsx! {
                li { id: format!("fn-{label}"), {self.render_children(children)} }
            },
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

    fn render_alert<Ext>(&self, kind: AlertKind, children: &[Node<Ext>]) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        let (slug, title) = match kind {
            AlertKind::Note => ("note", "Note"),
            AlertKind::Tip => ("tip", "Tip"),
            AlertKind::Important => ("important", "Important"),
            AlertKind::Warning => ("warning", "Warning"),
            AlertKind::Caution => ("caution", "Caution"),
        };
        rsx! {
            div { class: "markdown-alert markdown-alert-{slug}",
                p { class: "markdown-alert-title", "{title}" }
                {self.render_children(children)}
            }
        }
    }

    fn render_list<Ext>(&self, ordered: bool, start: Option<u64>, children: &[Node<Ext>]) -> Element
    where
        R: RenderDioxusCustom<Ext>,
    {
        if ordered {
            let start = start.unwrap_or(1);
            rsx! {
                ol { start: start as i64, {self.render_children(children)} }
            }
        } else {
            rsx! {
                ul { {self.render_children(children)} }
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
        rsx! {
            a {
                href: destination.to_string(),
                title: title.to_string(),
                {self.render_children(children)}
            }
        }
    }

    fn render_code_block(&self, info: Option<&str>, literal: &str) -> Element {
        // 1. pikchr (feature-gated)
        #[cfg(feature = "pikchr")]
        if self.options.pikchr && info == Some("pikchr") {
            let opts = pikru::RenderOptions {
                css_variables: true,
            };
            if let Ok(svg) = pikru::pikchr_with_options(literal, &opts) {
                return rsx! {
                    div { class: "pikchr", dangerous_inner_html: svg }
                };
            }
        }

        // 2. syntax highlighter (feature-gated)
        #[cfg(feature = "highlight")]
        if let Some(lang) = info {
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
        let class = match info {
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
    rsx! {
        img {
            src: destination.to_string(),
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
}
