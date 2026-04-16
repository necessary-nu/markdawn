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

use ::dioxus::prelude::*;

use crate::node::{AlertKind, Node, TableAlignment};
use crate::parser::Parser;

/// Parser configuration mirrored as a `Component`-friendly props type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DioxusOptions {
    pub gfm: bool,
    pub math: bool,
    pub alerts: bool,
    pub highlights: bool,
    pub definition_lists: bool,
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

/// Drop-in component for `rsx!` composition.
#[component]
pub fn Markdown(source: String, #[props(default)] options: DioxusOptions) -> Element {
    render_markdown(&source, options)
}

/// Parse `source` and render the resulting block list as a single `Element`.
pub fn render_markdown(source: &str, options: DioxusOptions) -> Element {
    let mut builder = Parser::<()>::builder();
    if options.gfm {
        builder = builder.with_gfm();
    }
    if options.math {
        builder = builder.with_math();
    }
    if options.alerts {
        builder = builder.with_alerts();
    }
    if options.highlights {
        builder = builder.with_highlights();
    }
    if options.definition_lists {
        builder = builder.with_definition_lists();
    }
    let mut parser = builder.build();
    let blocks = match parser.parse_full_at(source, 0) {
        Ok(blocks) => blocks,
        Err(_) => return rsx! {},
    };
    let nodes: Vec<Node<()>> = blocks.into_iter().map(|b| b.node).collect();
    rsx! {
        for node in nodes.iter() { {render_node(node, &())} }
    }
}

/// Render a single `Node` (block or inline) as a Dioxus `Element`.
pub fn render_node<Ext, R>(node: &Node<Ext>, custom: &R) -> Element
where
    R: RenderDioxusCustom<Ext>,
{
    match node {
        Node::Document { children, .. }
        | Node::FootnoteSection { children, .. }
        | Node::DefinitionList { children, .. } => render_children(children, custom),
        Node::Paragraph { children, .. } => rsx! {
            p { {render_children(children, custom)} }
        },
        Node::Heading {
            level, children, ..
        } => render_heading(*level, children, custom),
        Node::BlockQuote { children, .. } => rsx! {
            blockquote { {render_children(children, custom)} }
        },
        Node::Alert { kind, children, .. } => render_alert(*kind, children, custom),
        Node::List {
            ordered,
            start,
            children,
            ..
        } => render_list(*ordered, *start, children, custom),
        Node::Item { children, .. } => rsx! {
            li { {render_children(children, custom)} }
        },
        Node::TaskItem {
            checked, children, ..
        } => rsx! {
            li {
                input { r#type: "checkbox", disabled: true, checked: *checked }
                {render_children(children, custom)}
            }
        },
        Node::Table {
            alignments,
            header,
            rows,
            ..
        } => render_table(alignments, header, rows, custom),
        Node::TableRow { cells, .. } => rsx! {
            tr { {render_children(cells, custom)} }
        },
        Node::TableCell {
            children,
            header,
            alignment,
            ..
        } => render_table_cell(*header, *alignment, children, custom),
        Node::CodeBlock { info, literal, .. } => render_code_block(info.as_deref(), literal),
        Node::ThematicBreak { .. } => rsx! { hr {} },
        Node::Html { html, .. } => rsx! {
            div { dangerous_inner_html: html.clone() }
        },
        Node::Text { text, .. } => rsx! { "{text}" },
        Node::Emphasis { children, .. } => rsx! {
            em { {render_children(children, custom)} }
        },
        Node::Strong { children, .. } => rsx! {
            strong { {render_children(children, custom)} }
        },
        Node::Strikethrough { children, .. } => rsx! {
            del { {render_children(children, custom)} }
        },
        Node::Highlight { children, .. } => rsx! {
            mark { {render_children(children, custom)} }
        },
        Node::Subscript { children, .. } => rsx! {
            sub { {render_children(children, custom)} }
        },
        Node::Superscript { children, .. } => rsx! {
            sup { {render_children(children, custom)} }
        },
        Node::Link {
            destination,
            title,
            children,
            ..
        } => render_link(destination, title.as_deref(), children, custom),
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
            li { id: format!("fn-{label}"), {render_children(children, custom)} }
        },
        Node::DefinitionTerm { children, .. } => rsx! {
            dt { {render_children(children, custom)} }
        },
        Node::DefinitionDescription { children, .. } => rsx! {
            dd { {render_children(children, custom)} }
        },
        Node::Custom {
            payload, children, ..
        } => custom.render_custom(payload, children),
    }
}

fn render_children<Ext, R>(children: &[Node<Ext>], custom: &R) -> Element
where
    R: RenderDioxusCustom<Ext>,
{
    rsx! {
        for child in children.iter() { {render_node(child, custom)} }
    }
}

fn render_heading<Ext, R>(level: u8, children: &[Node<Ext>], custom: &R) -> Element
where
    R: RenderDioxusCustom<Ext>,
{
    match level {
        1 => rsx! { h1 { {render_children(children, custom)} } },
        2 => rsx! { h2 { {render_children(children, custom)} } },
        3 => rsx! { h3 { {render_children(children, custom)} } },
        4 => rsx! { h4 { {render_children(children, custom)} } },
        5 => rsx! { h5 { {render_children(children, custom)} } },
        _ => rsx! { h6 { {render_children(children, custom)} } },
    }
}

fn render_alert<Ext, R>(kind: AlertKind, children: &[Node<Ext>], custom: &R) -> Element
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
            {render_children(children, custom)}
        }
    }
}

fn render_list<Ext, R>(
    ordered: bool,
    start: Option<u64>,
    children: &[Node<Ext>],
    custom: &R,
) -> Element
where
    R: RenderDioxusCustom<Ext>,
{
    if ordered {
        let start = start.unwrap_or(1);
        rsx! {
            ol { start: start as i64, {render_children(children, custom)} }
        }
    } else {
        rsx! {
            ul { {render_children(children, custom)} }
        }
    }
}

fn render_table<Ext, R>(
    alignments: &[Option<TableAlignment>],
    header: &[Node<Ext>],
    rows: &[Node<Ext>],
    custom: &R,
) -> Element
where
    R: RenderDioxusCustom<Ext>,
{
    let _ = alignments; // alignment is on the cell, picked up there
    rsx! {
        table {
            if !header.is_empty() {
                thead { {render_children(header, custom)} }
            }
            tbody { {render_children(rows, custom)} }
        }
    }
}

fn render_table_cell<Ext, R>(
    header: bool,
    alignment: Option<TableAlignment>,
    children: &[Node<Ext>],
    custom: &R,
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
        rsx! { th { style: style, {render_children(children, custom)} } }
    } else {
        rsx! { td { style: style, {render_children(children, custom)} } }
    }
}

fn render_code_block(info: Option<&str>, literal: &str) -> Element {
    let class = match info {
        Some(lang) if !lang.is_empty() => format!("language-{lang}"),
        _ => String::new(),
    };
    rsx! {
        pre { code { class: class, "{literal}" } }
    }
}

fn render_link<Ext, R>(
    destination: &str,
    title: Option<&str>,
    children: &[Node<Ext>],
    custom: &R,
) -> Element
where
    R: RenderDioxusCustom<Ext>,
{
    let title = title.unwrap_or_default();
    rsx! {
        a { href: destination.to_string(), title: title.to_string(),
            {render_children(children, custom)}
        }
    }
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
}
