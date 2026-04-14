use core::fmt;

use crate::node::{Emission, Node, StreamItem, TableAlignment};
use crate::parser::filter_disallowed_html;
use crate::sink::Sink;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlError {
    UnsupportedCustomNode,
    Sink(String),
}

impl fmt::Display for HtmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCustomNode => f.write_str("custom node renderer not configured"),
            Self::Sink(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for HtmlError {}

pub trait RenderHtmlCustom<Ext, Meta> {
    fn render_custom(
        &self,
        payload: &Ext,
        children: &[Node<Ext, Meta>],
        out: &mut String,
    ) -> Result<(), HtmlError>;
}

impl<Ext, Meta> RenderHtmlCustom<Ext, Meta> for () {
    fn render_custom(
        &self,
        _payload: &Ext,
        _children: &[Node<Ext, Meta>],
        _out: &mut String,
    ) -> Result<(), HtmlError> {
        Err(HtmlError::UnsupportedCustomNode)
    }
}

pub struct HtmlAdapter<R = ()> {
    custom: R,
}

impl Default for HtmlAdapter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlAdapter<()> {
    pub fn new() -> Self {
        Self { custom: () }
    }

    pub fn with_custom_renderer<Custom>(custom: Custom) -> HtmlAdapter<Custom> {
        HtmlAdapter { custom }
    }
}

impl<R> HtmlAdapter<R> {
    pub fn render<Ext, Meta>(
        &self,
        item: StreamItem<Node<Ext, Meta>>,
    ) -> Result<StreamItem<String>, HtmlError>
    where
        R: RenderHtmlCustom<Ext, Meta>,
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

    fn render_node<Ext, Meta>(&self, node: &Node<Ext, Meta>) -> Result<String, HtmlError>
    where
        R: RenderHtmlCustom<Ext, Meta>,
    {
        let mut out = String::new();
        self.write_block(node, &mut out)?;
        Ok(out)
    }

    fn write_block<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext, Meta>,
    {
        match node {
            Node::Document { children, .. } => {
                for child in children {
                    self.write_block(child, out)?;
                }
            }
            Node::Paragraph { children, .. } => {
                out.push_str("<p>");
                self.write_inlines(children, out)?;
                out.push_str("</p>\n");
            }
            Node::Heading {
                level, children, ..
            } => {
                out.push_str("<h");
                out.push(char::from(b'0' + *level));
                out.push('>');
                self.write_inlines(children, out)?;
                out.push_str("</h");
                out.push(char::from(b'0' + *level));
                out.push_str(">\n");
            }
            Node::BlockQuote { children, .. } => {
                out.push_str("<blockquote>\n");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("</blockquote>\n");
            }
            Node::List {
                ordered,
                start,
                tight,
                children,
                ..
            } => {
                if *ordered {
                    match start {
                        Some(1) | None => out.push_str("<ol>\n"),
                        Some(start) => {
                            out.push_str("<ol start=\"");
                            out.push_str(&start.to_string());
                            out.push_str("\">\n");
                        }
                    }
                } else {
                    out.push_str("<ul>\n");
                }

                for child in children {
                    self.write_list_child(child, tight.unwrap_or(true), out)?;
                }

                if *ordered {
                    out.push_str("</ol>\n");
                } else {
                    out.push_str("</ul>\n");
                }
            }
            Node::Item { children, .. } => {
                out.push_str("<li>");
                if children.len() == 1 && matches!(children[0], Node::Paragraph { .. }) {
                    if let Node::Paragraph { children, .. } = &children[0] {
                        self.write_inlines(children, out)?;
                    }
                    out.push_str("</li>\n");
                } else {
                    out.push('\n');
                    for child in children {
                        self.write_block(child, out)?;
                    }
                    out.push_str("</li>\n");
                }
            }
            Node::TaskItem {
                checked, children, ..
            } => {
                out.push_str("<li><input type=\"checkbox\" disabled=\"\" ");
                if *checked {
                    out.push_str("checked=\"\" ");
                }
                out.push_str("/> ");
                if children.len() == 1 && matches!(children[0], Node::Paragraph { .. }) {
                    if let Node::Paragraph { children, .. } = &children[0] {
                        self.write_inlines(children, out)?;
                    }
                    out.push_str("</li>\n");
                } else {
                    out.push('\n');
                    for child in children {
                        self.write_block(child, out)?;
                    }
                    out.push_str("</li>\n");
                }
            }
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => {
                out.push_str("<table>\n<thead>\n");
                for row in header {
                    self.write_table_row(row, alignments, out)?;
                }
                out.push_str("</thead>\n<tbody>\n");
                for row in rows {
                    self.write_table_row(row, alignments, out)?;
                }
                out.push_str("</tbody>\n</table>\n");
            }
            Node::TableRow { .. } | Node::TableCell { .. } => {
                self.write_table_row(node, &[], out)?;
            }
            Node::CodeBlock { info, literal, .. } => {
                match info {
                    Some(info) => {
                        out.push_str("<pre><code class=\"language-");
                        escape_html(info, out);
                        out.push_str("\">");
                    }
                    None => out.push_str("<pre><code>"),
                }
                escape_html(literal, out);
                out.push_str("</code></pre>\n");
            }
            Node::Html { html, .. } => {
                if is_type1_html_block(html) {
                    out.push_str(html);
                } else {
                    out.push_str(&filter_disallowed_html(html));
                }
                if !html.ends_with('\n') {
                    out.push('\n');
                }
            }
            Node::ThematicBreak { .. } => out.push_str("<hr />\n"),
            Node::Custom {
                payload, children, ..
            } => {
                self.custom.render_custom(payload, children, out)?;
                if !out.ends_with('\n') && is_block_like(children) {
                    out.push('\n');
                }
            }
            Node::Text { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. } => self.write_inline(node, out)?,
        }

        Ok(())
    }

    fn write_inlines<Ext, Meta>(
        &self,
        nodes: &[Node<Ext, Meta>],
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext, Meta>,
    {
        for node in nodes {
            self.write_inline(node, out)?;
        }
        Ok(())
    }

    fn write_inline<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext, Meta>,
    {
        match node {
            Node::Text { text, .. } => escape_html(text, out),
            Node::Emphasis { children, .. } => {
                out.push_str("<em>");
                self.write_inlines(children, out)?;
                out.push_str("</em>");
            }
            Node::Strong { children, .. } => {
                out.push_str("<strong>");
                self.write_inlines(children, out)?;
                out.push_str("</strong>");
            }
            Node::Strikethrough { children, .. } => {
                out.push_str("<del>");
                self.write_inlines(children, out)?;
                out.push_str("</del>");
            }
            Node::Link {
                destination,
                title,
                children,
                ..
            } => {
                out.push_str("<a href=\"");
                escape_href(destination, out);
                out.push('"');
                if let Some(title) = title {
                    out.push_str(" title=\"");
                    escape_html(title, out);
                    out.push('"');
                }
                out.push('>');
                self.write_inlines(children, out)?;
                out.push_str("</a>");
            }
            Node::Image {
                destination,
                title,
                alt,
                ..
            } => {
                out.push_str("<img src=\"");
                escape_href(destination, out);
                out.push_str("\" alt=\"");
                escape_html(alt, out);
                out.push('"');
                if let Some(title) = title {
                    out.push_str(" title=\"");
                    escape_html(title, out);
                    out.push('"');
                }
                out.push_str(" />");
            }
            Node::CodeSpan { code, .. } => {
                out.push_str("<code>");
                escape_html(code, out);
                out.push_str("</code>");
            }
            Node::Html { html, .. } => out.push_str(html),
            Node::SoftBreak { .. } => out.push('\n'),
            Node::HardBreak { .. } => out.push_str("<br />\n"),
            Node::Custom {
                payload, children, ..
            } => self.custom.render_custom(payload, children, out)?,
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
            | Node::ThematicBreak { .. }
            | Node::Document { .. } => self.write_block(node, out)?,
        }

        Ok(())
    }

    fn write_table_row<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        alignments: &[Option<TableAlignment>],
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext, Meta>,
    {
        let Node::TableRow { cells, .. } = node else {
            return Ok(());
        };
        out.push_str("<tr>");
        for (index, cell) in cells.iter().enumerate() {
            let Node::TableCell {
                children,
                header,
                alignment,
                ..
            } = cell
            else {
                continue;
            };
            let tag = if *header { "th" } else { "td" };
            out.push('<');
            out.push_str(tag);
            let align = alignment.or_else(|| alignments.get(index).copied().flatten());
            if let Some(align) = align {
                out.push_str(" align=\"");
                out.push_str(match align {
                    TableAlignment::Left => "left",
                    TableAlignment::Center => "center",
                    TableAlignment::Right => "right",
                });
                out.push('"');
            }
            out.push('>');
            self.write_inlines(children, out)?;
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        out.push_str("</tr>\n");
        Ok(())
    }

    fn write_list_child<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        tight: bool,
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext, Meta>,
    {
        match node {
            Node::Item { children, .. } => {
                out.push_str("<li>");
                if children.is_empty() {
                    out.push_str("</li>\n");
                } else if tight && matches!(children.first(), Some(Node::Paragraph { .. })) {
                    if let Node::Paragraph { children, .. } = &children[0] {
                        self.write_inlines(children, out)?;
                    }
                    for child in &children[1..] {
                        self.write_block(child, out)?;
                    }
                    out.push_str("</li>\n");
                } else {
                    out.push('\n');
                    for child in children {
                        self.write_block(child, out)?;
                    }
                    out.push_str("</li>\n");
                }
            }
            Node::TaskItem {
                checked, children, ..
            } => {
                out.push_str("<li><input type=\"checkbox\" disabled=\"\" ");
                if *checked {
                    out.push_str("checked=\"\" ");
                }
                out.push_str("/> ");
                if children.is_empty() {
                    out.push_str("</li>\n");
                } else if tight && matches!(children.first(), Some(Node::Paragraph { .. })) {
                    if let Node::Paragraph { children, .. } = &children[0] {
                        self.write_inlines(children, out)?;
                    }
                    for child in &children[1..] {
                        self.write_block(child, out)?;
                    }
                    out.push_str("</li>\n");
                } else {
                    out.push('\n');
                    for child in children {
                        self.write_block(child, out)?;
                    }
                    out.push_str("</li>\n");
                }
            }
            other => self.write_block(other, out)?,
        }

        Ok(())
    }
}

pub struct HtmlSink<S, R = ()> {
    inner: S,
    adapter: HtmlAdapter<R>,
}

impl<S> HtmlSink<S, ()> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            adapter: HtmlAdapter::new(),
        }
    }
}

impl<S, R> HtmlSink<S, R> {
    pub fn with_custom_renderer(inner: S, custom: R) -> Self {
        Self {
            inner,
            adapter: HtmlAdapter { custom },
        }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, R, Ext, Meta> Sink<Node<Ext, Meta>> for HtmlSink<S, R>
where
    S: Sink<String>,
    S::Error: fmt::Display,
    R: RenderHtmlCustom<Ext, Meta>,
{
    type Error = HtmlError;

    fn emit(&mut self, item: StreamItem<Node<Ext, Meta>>) -> Result<(), Self::Error> {
        let rendered = self.adapter.render(item)?;
        self.inner
            .emit(rendered)
            .map_err(|error| HtmlError::Sink(error.to_string()))
    }
}

fn escape_href(url: &str, out: &mut String) {
    use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

    // Encode everything except URL-safe characters and already-encoded %XX
    const URL_SAFE: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~')
        .remove(b':')
        .remove(b'/')
        .remove(b'?')
        .remove(b'#')
        .remove(b'[')
        .remove(b']')
        .remove(b'@')
        .remove(b'!')
        .remove(b'$')
        .remove(b'&')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')')
        .remove(b'*')
        .remove(b'+')
        .remove(b',')
        .remove(b';')
        .remove(b'=')
        .remove(b'%');

    out.push_str(&utf8_percent_encode(url, URL_SAFE).to_string());
}

fn escape_html(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// Type 1 HTML block tags (pre, script, style, textarea) are pass-through
/// blocks whose content is raw HTML.  The disallowed-tag filter must NOT
/// mangle these because the tag itself forms the block.
fn is_type1_html_block(html: &str) -> bool {
    const TYPE1_TAGS: &[&str] = &["pre", "script", "style", "textarea"];
    let trimmed = html.trim_start();
    for tag in TYPE1_TAGS {
        let open = format!("<{tag}");
        if trimmed.len() >= open.len() {
            let candidate = &trimmed[..open.len()];
            if candidate.eq_ignore_ascii_case(&open) {
                let after = trimmed[open.len()..].chars().next();
                if after.is_none_or(|ch| matches!(ch, ' ' | '\t' | '>' | '\n')) {
                    return true;
                }
            }
        }
    }
    false
}

fn is_block_like<Ext, Meta>(children: &[Node<Ext, Meta>]) -> bool {
    children.iter().any(|child| {
        matches!(
            child,
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
                | Node::Html { .. }
                | Node::ThematicBreak { .. }
                | Node::Document { .. }
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::adapter::html::{HtmlAdapter, HtmlSink, RenderHtmlCustom};
    use crate::node::{Emission, Node, StreamItem};
    use crate::sink::{Sink, VecSink};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestExt {
        Math(String),
    }

    struct MathRenderer;

    impl RenderHtmlCustom<TestExt, ()> for MathRenderer {
        fn render_custom(
            &self,
            payload: &TestExt,
            _children: &[Node<TestExt, ()>],
            out: &mut String,
        ) -> Result<(), super::HtmlError> {
            match payload {
                TestExt::Math(source) => {
                    out.push_str("<span class=\"math\">");
                    out.push_str(source);
                    out.push_str("</span>");
                    Ok(())
                }
            }
        }
    }

    #[test]
    fn renders_html_upsert_with_same_id_and_provisional_flag() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 7,
                provisional: true,
                node: Node::<(), ()>::Paragraph {
                    children: vec![Node::Text {
                        text: "hello".to_owned(),
                        meta: (),
                    }],
                    meta: (),
                },
            }))
            .unwrap();

        assert_eq!(
            rendered,
            StreamItem::Upsert(Emission {
                id: 7,
                provisional: true,
                node: "<p>hello</p>\n".to_owned(),
            })
        );
    }

    #[test]
    fn passes_delete_through_unchanged() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::<Node<(), ()>>::Delete { id: 9 })
            .unwrap();
        assert_eq!(rendered, StreamItem::Delete { id: 9 });
    }

    #[test]
    fn renders_lists_and_block_quotes() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 1,
                provisional: false,
                node: Node::<(), ()>::BlockQuote {
                    children: vec![Node::List {
                        ordered: false,
                        start: None,
                        tight: None,
                        children: vec![Node::Item {
                            children: vec![Node::Paragraph {
                                children: vec![Node::Text {
                                    text: "item".to_owned(),
                                    meta: (),
                                }],
                                meta: (),
                            }],
                            meta: (),
                        }],
                        meta: (),
                    }],
                    meta: (),
                },
            }))
            .unwrap();

        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert_eq!(
            emission.node,
            "<blockquote>\n<ul>\n<li>item</li>\n</ul>\n</blockquote>\n"
        );
    }

    #[test]
    fn html_sink_wraps_existing_sink() {
        let inner = VecSink::<String>::new();
        let mut sink = HtmlSink::new(inner);

        sink.emit(StreamItem::Upsert(Emission {
            id: 11,
            provisional: false,
            node: Node::<(), ()>::ThematicBreak { meta: () },
        }))
        .unwrap();

        let inner = sink.into_inner();
        assert_eq!(
            inner.items(),
            &[StreamItem::Upsert(Emission {
                id: 11,
                provisional: false,
                node: "<hr />\n".to_owned(),
            })]
        );
    }

    #[test]
    fn custom_renderer_handles_custom_nodes() {
        let adapter = HtmlAdapter::<()>::with_custom_renderer(MathRenderer);
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 3,
                provisional: false,
                node: Node::<TestExt, ()>::Paragraph {
                    children: vec![Node::Custom {
                        payload: TestExt::Math("x^2".to_owned()),
                        children: Vec::new(),
                        meta: (),
                    }],
                    meta: (),
                },
            }))
            .unwrap();

        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert_eq!(emission.node, "<p><span class=\"math\">x^2</span></p>\n");
    }
}
