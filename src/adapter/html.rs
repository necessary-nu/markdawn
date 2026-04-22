use core::fmt;

use crate::node::{AlertKind, Emission, Node, StreamItem, TableAlignment};
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

pub trait RenderHtmlCustom<Ext> {
    fn render_custom(
        &self,
        payload: &Ext,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), HtmlError>;
}

impl<Ext> RenderHtmlCustom<Ext> for () {
    fn render_custom(
        &self,
        _payload: &Ext,
        _children: &[Node<Ext>],
        _out: &mut String,
    ) -> Result<(), HtmlError> {
        Err(HtmlError::UnsupportedCustomNode)
    }
}

pub struct HtmlAdapter<R = ()> {
    custom: R,
    html_filter: crate::HtmlFilter,
    #[cfg(feature = "highlight")]
    highlighter: core::cell::RefCell<Option<Box<dyn crate::highlight::SpanHighlighter>>>,
    #[cfg(feature = "pikchr")]
    pikchr: bool,
    #[cfg(feature = "graphviz")]
    graphviz: bool,
}

impl Default for HtmlAdapter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlAdapter<()> {
    pub fn new() -> Self {
        Self {
            custom: (),
            html_filter: crate::HtmlFilter::safe(),
            #[cfg(feature = "highlight")]
            highlighter: core::cell::RefCell::new(None),
            #[cfg(feature = "pikchr")]
            pikchr: false,
            #[cfg(feature = "graphviz")]
            graphviz: false,
        }
    }

    pub fn with_custom_renderer<Custom>(custom: Custom) -> HtmlAdapter<Custom> {
        HtmlAdapter {
            custom,
            html_filter: crate::HtmlFilter::safe(),
            #[cfg(feature = "highlight")]
            highlighter: core::cell::RefCell::new(None),
            #[cfg(feature = "pikchr")]
            pikchr: false,
            #[cfg(feature = "graphviz")]
            graphviz: false,
        }
    }
}

impl<R> HtmlAdapter<R> {
    /// Override the HTML tag filter applied to every `Node::Html` (and
    /// every block-level type 1 HTML block — we no longer pass type 1
    /// blocks through unfiltered). Default is [`HtmlFilter::safe`].
    /// Pass [`HtmlFilter::unrestricted`] to opt in to raw HTML.
    pub fn with_html_filter(mut self, filter: crate::HtmlFilter) -> Self {
        self.html_filter = filter;
        self
    }
}

#[cfg(feature = "highlight")]
impl<R> HtmlAdapter<R> {
    /// Install a syntax highlighter. Any `arborium::advanced::HighlightSpans`
    /// implementation works. Failures in highlighting fall back to plain
    /// `<pre><code>` rendering.
    pub fn with_highlighter<H>(mut self, highlighter: H) -> Self
    where
        H: arborium::advanced::HighlightSpans + 'static,
    {
        self.highlighter = core::cell::RefCell::new(Some(Box::new(highlighter)));
        self
    }
}

#[cfg(feature = "pikchr")]
impl<R> HtmlAdapter<R> {
    /// Render fenced code blocks tagged with the `pikchr` info string as
    /// inline SVG diagrams. Parse failures fall back to plain `<pre><code>`.
    pub fn with_pikchr(mut self) -> Self {
        self.pikchr = true;
        self
    }
}

#[cfg(feature = "graphviz")]
impl<R> HtmlAdapter<R> {
    /// Render fenced code blocks tagged `dot` or `graphviz` as inline SVG
    /// diagrams by shelling into libgraphviz via `graphviz-sys`. Parse /
    /// layout failures fall back to plain `<pre><code>`.
    ///
    /// Requires the Graphviz C libraries (`libgvc`, `libcgraph`) available
    /// at build time (`brew install graphviz` on macOS; the `graphviz-dev`
    /// package on Debian/Ubuntu).
    pub fn with_graphviz(mut self) -> Self {
        self.graphviz = true;
        self
    }
}

impl<R> HtmlAdapter<R> {
    pub fn render<Ext>(&self, item: StreamItem<Node<Ext>>) -> Result<StreamItem<String>, HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
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

    fn render_node<Ext>(&self, node: &Node<Ext>) -> Result<String, HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
    {
        let mut out = String::new();
        self.write_block(node, &mut out)?;
        Ok(out)
    }

    /// Returns `true` if the code block was rendered with syntax highlighting.
    #[cfg(feature = "highlight")]
    fn try_highlight_code_block(
        &self,
        info: Option<&str>,
        literal: &str,
        out: &mut String,
    ) -> Result<bool, HtmlError> {
        let Some(lang) = info
            .and_then(|s| s.split_whitespace().next())
            .filter(|s| !s.is_empty())
        else {
            return Ok(false);
        };
        let mut borrow = self.highlighter.borrow_mut();
        let Some(hl) = borrow.as_mut() else {
            return Ok(false);
        };
        let Some(spans) = hl.spans(lang, literal) else {
            return Ok(false);
        };
        out.push_str("<pre><code class=\"language-");
        escape_html(lang, out);
        out.push_str("\">");
        let html = arborium::advanced::spans_to_html(
            literal,
            spans,
            &arborium::advanced::HtmlFormat::default(),
        );
        out.push_str(&html);
        out.push_str("</code></pre>\n");
        Ok(true)
    }

    #[cfg(not(feature = "highlight"))]
    fn try_highlight_code_block(
        &self,
        _info: Option<&str>,
        _literal: &str,
        _out: &mut String,
    ) -> Result<bool, HtmlError> {
        Ok(false)
    }

    /// Returns `true` if the block was rendered as a pikchr SVG diagram.
    #[cfg(feature = "pikchr")]
    fn try_pikchr_code_block(&self, info: Option<&str>, literal: &str, out: &mut String) -> bool {
        let lang = info.and_then(|s| s.split_whitespace().next());
        if !self.pikchr || lang != Some("pikchr") {
            return false;
        }
        let Ok(svg) = crate::pikchr_render::render(literal) else {
            return false;
        };
        if svg.len() > crate::svg_sanitize::MAX_SVG_BYTES {
            return false;
        }
        let sanitized = crate::svg_sanitize::sanitize_svg(&svg);
        out.push_str(&sanitized);
        if !sanitized.ends_with('\n') {
            out.push('\n');
        }
        true
    }

    #[cfg(not(feature = "pikchr"))]
    fn try_pikchr_code_block(
        &self,
        _info: Option<&str>,
        _literal: &str,
        _out: &mut String,
    ) -> bool {
        false
    }

    /// Returns `true` if the block was rendered as a graphviz SVG diagram.
    #[cfg(feature = "graphviz")]
    fn try_graphviz_code_block(&self, info: Option<&str>, literal: &str, out: &mut String) -> bool {
        let lang = info.and_then(|s| s.split_whitespace().next());
        if !self.graphviz || !matches!(lang, Some("dot") | Some("graphviz")) {
            return false;
        }
        let Ok(svg) = markdawn_graphviz::render_dot_to_svg(literal) else {
            return false;
        };
        if svg.len() > crate::svg_sanitize::MAX_SVG_BYTES {
            return false;
        }
        let sanitized = crate::svg_sanitize::sanitize_svg(&svg);
        out.push_str(&sanitized);
        if !sanitized.ends_with('\n') {
            out.push('\n');
        }
        true
    }

    #[cfg(not(feature = "graphviz"))]
    fn try_graphviz_code_block(
        &self,
        _info: Option<&str>,
        _literal: &str,
        _out: &mut String,
    ) -> bool {
        false
    }

    fn write_block<Ext>(&self, node: &Node<Ext>, out: &mut String) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
    {
        match node {
            Node::Document { children, .. } => {
                for child in children {
                    self.write_block(child, out)?;
                }
            }
            Node::FootnoteSection { children, .. } => {
                if children.is_empty() {
                    return Ok(());
                }
                out.push_str("<section class=\"footnotes\" data-footnotes>\n<ol>\n");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("</ol>\n</section>\n");
            }
            Node::FootnoteDefinition {
                label,
                number,
                backref_count,
                children,
                ..
            } => {
                out.push_str("<li id=\"fn-");
                escape_href(label, out);
                out.push_str("\">\n");
                if children.is_empty() {
                    self.write_footnote_backrefs(label, *number, *backref_count, out);
                    out.push('\n');
                } else {
                    let last_idx = children.len() - 1;
                    for (idx, child) in children.iter().enumerate() {
                        if idx == last_idx {
                            if let Node::Paragraph {
                                children: inlines, ..
                            } = child
                            {
                                out.push_str("<p>");
                                self.write_inlines(inlines, out)?;
                                out.push(' ');
                                self.write_footnote_backrefs(label, *number, *backref_count, out);
                                out.push_str("</p>\n");
                            } else {
                                self.write_block(child, out)?;
                                self.write_footnote_backrefs(label, *number, *backref_count, out);
                                out.push('\n');
                            }
                        } else {
                            self.write_block(child, out)?;
                        }
                    }
                }
                out.push_str("</li>\n");
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
                if rows.is_empty() {
                    out.push_str("</thead>\n</table>\n");
                } else {
                    out.push_str("</thead>\n<tbody>\n");
                    for row in rows {
                        self.write_table_row(row, alignments, out)?;
                    }
                    out.push_str("</tbody>\n</table>\n");
                }
            }
            Node::TableRow { .. } | Node::TableCell { .. } => {
                self.write_table_row(node, &[], out)?;
            }
            Node::CodeBlock { info, literal, .. } => {
                let lang = info
                    .as_deref()
                    .and_then(|s| s.split_whitespace().next())
                    .filter(|s| !s.is_empty());
                if matches!(lang, Some("ansi")) {
                    write_ansi_block(literal, out);
                } else if matches!(lang, Some("terminal")) {
                    write_terminal_block(info.as_deref(), literal, out);
                } else if !self.try_pikchr_code_block(info.as_deref(), literal, out)
                    && !self.try_graphviz_code_block(info.as_deref(), literal, out)
                    && !self.try_highlight_code_block(info.as_deref(), literal, out)?
                {
                    match lang {
                        Some(lang) => {
                            out.push_str("<pre><code class=\"language-");
                            escape_html(lang, out);
                            out.push_str("\">");
                        }
                        None => out.push_str("<pre><code>"),
                    }
                    escape_html(literal, out);
                    out.push_str("</code></pre>\n");
                }
            }
            Node::MathBlock { math, .. } => {
                out.push_str("<pre><code class=\"language-math math-display\">");
                escape_html(math, out);
                out.push_str("</code></pre>\n");
            }
            Node::Html { html, .. } => {
                // Always filter, regardless of GFM-spec type 1 exception.
                // Consumers who really want raw HTML configure
                // `HtmlFilter::unrestricted` on the adapter.
                out.push_str(&self.html_filter.filter(html));
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
            Node::Alert {
                kind,
                title,
                children,
                ..
            } => {
                let (slug, default_title) = match kind {
                    AlertKind::Note => ("note", "Note"),
                    AlertKind::Tip => ("tip", "Tip"),
                    AlertKind::Important => ("important", "Important"),
                    AlertKind::Warning => ("warning", "Warning"),
                    AlertKind::Caution => ("caution", "Caution"),
                };
                let shown_title = title.as_deref().unwrap_or(default_title);
                out.push_str("<div class=\"markdown-alert markdown-alert-");
                out.push_str(slug);
                out.push_str("\">\n<p class=\"markdown-alert-title\">");
                escape_html(shown_title, out);
                out.push_str("</p>\n");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("</div>\n");
            }
            Node::DefinitionList { children, .. } => {
                out.push_str("<dl>\n");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("</dl>\n");
            }
            Node::DefinitionTerm { children, .. } => {
                out.push_str("<dt>");
                self.write_inlines(children, out)?;
                out.push_str("</dt>\n");
            }
            Node::DefinitionDescription { children, .. } => {
                out.push_str("<dd>");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("</dd>\n");
            }
            Node::FootnoteRef { .. } => self.write_inline(node, out)?,
            Node::MathInline { .. } => self.write_inline(node, out)?,
            Node::Text { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Subscript { .. }
            | Node::Superscript { .. }
            | Node::Highlight { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. } => self.write_inline(node, out)?,
        }

        Ok(())
    }

    fn write_inlines<Ext>(&self, nodes: &[Node<Ext>], out: &mut String) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
    {
        for node in nodes {
            self.write_inline(node, out)?;
        }
        Ok(())
    }

    fn write_inline<Ext>(&self, node: &Node<Ext>, out: &mut String) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
    {
        match node {
            Node::Text { text, .. } => escape_html(text, out),
            Node::FootnoteRef {
                label,
                number,
                ref_num,
                ..
            } => {
                out.push_str("<sup class=\"footnote-ref\"><a href=\"#fn-");
                escape_href(label, out);
                out.push_str("\" id=\"fnref-");
                escape_href(label, out);
                if *ref_num > 1 {
                    out.push('-');
                    out.push_str(&ref_num.to_string());
                }
                out.push_str("\" data-footnote-ref>");
                out.push_str(&number.to_string());
                out.push_str("</a></sup>");
            }
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
            Node::Subscript { children, .. } => {
                out.push_str("<sub>");
                self.write_inlines(children, out)?;
                out.push_str("</sub>");
            }
            Node::Superscript { children, .. } => {
                out.push_str("<sup>");
                self.write_inlines(children, out)?;
                out.push_str("</sup>");
            }
            Node::Highlight { children, .. } => {
                out.push_str("<mark>");
                self.write_inlines(children, out)?;
                out.push_str("</mark>");
            }
            Node::Link {
                destination,
                title,
                children,
                ..
            } => {
                out.push_str("<a href=\"");
                if crate::url_safety::is_safe_url(destination) {
                    escape_href(destination, out);
                }
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
                if crate::url_safety::is_safe_url(destination) {
                    escape_href(destination, out);
                }
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
            Node::MathInline { math, .. } => {
                out.push_str("<code class=\"language-math math-inline\">");
                escape_html(math, out);
                out.push_str("</code>");
            }
            Node::MathBlock { math, .. } => {
                // Display math encountered in an inline position (e.g. `$$x^2$$`
                // in the middle of a paragraph). Emit as an inline-valid `<code>`
                // with `math-display` so renderers still style it as display math.
                out.push_str("<code class=\"language-math math-display\">");
                escape_html(math, out);
                out.push_str("</code>");
            }
            Node::Html { html, .. } => out.push_str(&self.html_filter.filter(html)),
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
            | Node::FootnoteDefinition { .. }
            | Node::FootnoteSection { .. }
            | Node::Alert { .. }
            | Node::DefinitionList { .. }
            | Node::DefinitionTerm { .. }
            | Node::DefinitionDescription { .. }
            | Node::Document { .. } => self.write_block(node, out)?,
        }

        Ok(())
    }

    fn write_table_row<Ext>(
        &self,
        node: &Node<Ext>,
        alignments: &[Option<TableAlignment>],
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
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

    fn write_list_child<Ext>(
        &self,
        node: &Node<Ext>,
        tight: bool,
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
    {
        match node {
            Node::Item { children, .. } => {
                out.push_str("<li>");
                self.write_li_children(children, tight, out)?;
            }
            Node::TaskItem {
                checked, children, ..
            } => {
                out.push_str("<li><input type=\"checkbox\" disabled=\"\" ");
                if *checked {
                    out.push_str("checked=\"\" ");
                }
                out.push_str("/> ");
                self.write_li_children(children, tight, out)?;
            }
            other => self.write_block(other, out)?,
        }

        Ok(())
    }

    fn write_li_children<Ext>(
        &self,
        children: &[Node<Ext>],
        tight: bool,
        out: &mut String,
    ) -> Result<(), HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
    {
        if children.is_empty() {
            out.push_str("</li>\n");
            return Ok(());
        }
        if tight {
            if !matches!(children.first(), Some(Node::Paragraph { .. })) {
                out.push('\n');
            }
            for (i, child) in children.iter().enumerate() {
                if let Node::Paragraph {
                    children: inlines, ..
                } = child
                {
                    self.write_inlines(inlines, out)?;
                    if i + 1 < children.len() && matches!(children[i + 1], Node::Paragraph { .. }) {
                        out.push('\n');
                    }
                } else {
                    self.write_block(child, out)?;
                }
            }
            out.push_str("</li>\n");
        } else {
            out.push('\n');
            for child in children {
                self.write_block(child, out)?;
            }
            out.push_str("</li>\n");
        }
        Ok(())
    }

    fn write_footnote_backrefs(&self, label: &str, number: u32, count: u32, out: &mut String) {
        let count = count.max(1);
        for i in 1..=count {
            if i > 1 {
                out.push(' ');
            }
            out.push_str("<a href=\"#fnref-");
            escape_href(label, out);
            if i > 1 {
                out.push('-');
                out.push_str(&i.to_string());
            }
            out.push_str(
                "\" class=\"footnote-backref\" data-footnote-backref data-footnote-backref-idx=\"",
            );
            out.push_str(&number.to_string());
            if i > 1 {
                out.push('-');
                out.push_str(&i.to_string());
            }
            out.push_str("\" aria-label=\"Back to reference ");
            out.push_str(&number.to_string());
            if i > 1 {
                out.push('-');
                out.push_str(&i.to_string());
            }
            out.push_str("\">↩");
            if i > 1 {
                out.push_str("<sup class=\"footnote-ref\">");
                out.push_str(&i.to_string());
                out.push_str("</sup>");
            }
            out.push_str("</a>");
        }
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
            adapter: HtmlAdapter {
                custom,
                html_filter: crate::HtmlFilter::safe(),
                #[cfg(feature = "highlight")]
                highlighter: core::cell::RefCell::new(None),
                #[cfg(feature = "pikchr")]
                pikchr: false,
                #[cfg(feature = "graphviz")]
                graphviz: false,
            },
        }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, R, Ext> Sink<Node<Ext>> for HtmlSink<S, R>
where
    S: Sink<String>,
    S::Error: fmt::Display,
    R: RenderHtmlCustom<Ext>,
{
    type Error = HtmlError;

    fn emit(&mut self, item: StreamItem<Node<Ext>>) -> Result<(), Self::Error> {
        let rendered = self.adapter.render(item)?;
        self.inner
            .emit(rendered)
            .map_err(|error| HtmlError::Sink(error.to_string()))
    }
}

fn escape_href(url: &str, out: &mut String) {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

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

/// Emit a CSS-only toggleable two-pane wrapper for a `` ```ansi ``
/// code block. The rendered pane shows SGR-parsed colored spans; the
/// source pane shows the raw content with escape sequences annotated.
/// A hidden checkbox + sibling selector flips between the two — no JS.
///
/// Only available with the `ansi` Cargo feature.
#[cfg(feature = "ansi")]
fn write_ansi_block(literal: &str, out: &mut String) {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    out.push_str("<div class=\"ansi-block\">");
    // Self-close the <input> so the same markup is valid in both HTML5
    // and XHTML (ePub readers parse strict XHTML and choke on open tags).
    out.push_str(&format!(
        "<input type=\"checkbox\" id=\"ansi-src-{id}\" class=\"ansi-toggle-input\" aria-hidden=\"true\" />"
    ));
    out.push_str(&format!(
        "<label for=\"ansi-src-{id}\" class=\"ansi-toggle\"><span class=\"ansi-toggle-show\">show source</span><span class=\"ansi-toggle-hide\">show rendered</span></label>"
    ));
    out.push_str("<pre class=\"ansi-rendered\"><code class=\"language-ansi\">");
    out.push_str(&crate::ansi_to_html::ansi_to_html(literal));
    out.push_str("</code></pre>");
    out.push_str("<pre class=\"ansi-source\"><code class=\"language-ansi-source\">");
    out.push_str(&crate::ansi_to_html::ansi_to_annotated_source(literal));
    out.push_str("</code></pre>");
    out.push_str("</div>\n");
}

/// Fallback without the `ansi` feature: plain `<pre><code class="language-ansi">` with escapes visible as literal text.
#[cfg(not(feature = "ansi"))]
fn write_ansi_block(literal: &str, out: &mut String) {
    out.push_str("<pre><code class=\"language-ansi\">");
    escape_html(literal, out);
    out.push_str("</code></pre>\n");
}

/// Emit a terminal-snapshot block. Feeds `literal` through a VT100
/// emulator and renders the final visible screen, producing the same
/// two-pane "rendered / show source" toggle as [`write_ansi_block`].
/// The info-string MAY carry `cols=N rows=N` suffixes to pin the grid;
/// defaults auto-size (80 cols, auto-trimmed rows).
///
/// Only available with the `terminal` Cargo feature.
#[cfg(feature = "terminal")]
fn write_terminal_block(info: Option<&str>, literal: &str, out: &mut String) {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    let (cols, rows) = parse_terminal_geometry(info);

    out.push_str("<div class=\"terminal-block\">");
    out.push_str(&format!(
        "<input type=\"checkbox\" id=\"terminal-src-{id}\" class=\"terminal-toggle-input\" aria-hidden=\"true\" />"
    ));
    out.push_str(&format!(
        "<label for=\"terminal-src-{id}\" class=\"terminal-toggle\"><span class=\"terminal-toggle-show\">show source</span><span class=\"terminal-toggle-hide\">show rendered</span></label>"
    ));
    out.push_str("<pre class=\"terminal-rendered\"><code class=\"language-terminal\">");
    out.push_str(&crate::terminal::ansi_snapshot_to_html(literal, cols, rows));
    out.push_str("</code></pre>");
    out.push_str("<pre class=\"terminal-source\"><code class=\"language-terminal-source\">");
    out.push_str(&crate::ansi_to_html::ansi_to_annotated_source(literal));
    out.push_str("</code></pre>");
    out.push_str("</div>\n");
}

/// Fallback without the `terminal` feature: same as `ansi` — just show
/// the literal bytes with escapes visible as text.
#[cfg(not(feature = "terminal"))]
fn write_terminal_block(_info: Option<&str>, literal: &str, out: &mut String) {
    out.push_str("<pre><code class=\"language-terminal\">");
    escape_html(literal, out);
    out.push_str("</code></pre>\n");
}

/// Parse `cols=N` / `rows=N` out of an info-string like
/// `terminal cols=120 rows=40`. Unknown tokens are ignored; missing
/// values fall back to the auto-sizing path in
/// [`crate::terminal::ansi_snapshot_to_html`].
#[cfg(feature = "terminal")]
fn parse_terminal_geometry(info: Option<&str>) -> (Option<u16>, Option<u16>) {
    let mut cols = None;
    let mut rows = None;
    if let Some(info) = info {
        for token in info.split_whitespace().skip(1) {
            if let Some(val) = token.strip_prefix("cols=") {
                cols = val.parse().ok();
            } else if let Some(val) = token.strip_prefix("rows=") {
                rows = val.parse().ok();
            }
        }
    }
    (cols, rows)
}

fn is_block_like<Ext>(children: &[Node<Ext>]) -> bool {
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
                | Node::MathBlock { .. }
                | Node::Html { .. }
                | Node::ThematicBreak { .. }
                | Node::Document { .. }
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::adapter::html::{HtmlAdapter, HtmlSink, RenderHtmlCustom};
    use crate::node::{Emission, Node, SourceSpan, StreamItem};
    use crate::sink::{Sink, VecSink};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestExt {
        Math(String),
    }

    struct MathRenderer;

    impl RenderHtmlCustom<TestExt> for MathRenderer {
        fn render_custom(
            &self,
            payload: &TestExt,
            _children: &[Node<TestExt>],
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
                node: Node::<()>::Paragraph {
                    children: vec![Node::Text {
                        text: "hello".to_owned(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
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
            .render(StreamItem::<Node<()>>::Delete { id: 9 })
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
                node: Node::<()>::BlockQuote {
                    children: vec![Node::List {
                        ordered: false,
                        start: None,
                        tight: None,
                        children: vec![Node::Item {
                            children: vec![Node::Paragraph {
                                children: vec![Node::Text {
                                    text: "item".to_owned(),
                                    source: SourceSpan::default(),
                                }],
                                source: SourceSpan::default(),
                            }],
                            source: SourceSpan::default(),
                        }],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
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
            node: Node::<()>::ThematicBreak {
                source: SourceSpan::default(),
            },
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

    // adv[verify markdown.url-allowlist]
    #[test]
    fn javascript_link_destination_is_stripped() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 1,
                provisional: false,
                node: Node::<()>::Paragraph {
                    children: vec![Node::Link {
                        destination: "javascript:alert(1)".to_owned(),
                        title: None,
                        children: vec![Node::Text {
                            text: "x".to_owned(),
                            source: SourceSpan::default(),
                        }],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();
        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert!(!emission.node.contains("javascript:"), "got: {}", emission.node);
        assert!(emission.node.contains("href=\"\""), "got: {}", emission.node);
    }

    #[test]
    fn javascript_image_destination_is_stripped() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 1,
                provisional: false,
                node: Node::<()>::Paragraph {
                    children: vec![Node::Image {
                        destination: "javascript:alert(1)".to_owned(),
                        title: None,
                        alt: "x".to_owned(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();
        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert!(!emission.node.contains("javascript:"), "got: {}", emission.node);
    }

    #[test]
    fn custom_renderer_handles_custom_nodes() {
        let adapter = HtmlAdapter::<()>::with_custom_renderer(MathRenderer);
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 3,
                provisional: false,
                node: Node::<TestExt>::Paragraph {
                    children: vec![Node::Custom {
                        payload: TestExt::Math("x^2".to_owned()),
                        children: Vec::new(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();

        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert_eq!(emission.node, "<p><span class=\"math\">x^2</span></p>\n");
    }
}
