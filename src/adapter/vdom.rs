//! Canonical renderer: `Node` → a framework-neutral `VNode` tree.
//!
//! This is the single source of truth for how a markdown node maps to
//! output structure. The HTML adapter (`adapter::html`) is a thin
//! serializer over the `VNode` tree produced here; the wasm-bindgen
//! crate serializes the same tree (via serde) for a Preact/React/Solid
//! frontend.
//!
//! The tree carries a few HTML-serialization hints (`nl_open`,
//! `nl_close`, `void`) that the HTML serializer consumes to reproduce
//! exact whitespace; they are skipped when serializing to JS.

use core::fmt;

use crate::node::{AlertKind, Emission, Node, StreamItem, TableAlignment};
use crate::sink::Sink;

// ----- VNode model -------------------------------------------------

/// A typed attribute value. Each target renders it its own way: the
/// HTML serializer escapes/encodes per variant, while a JS frontend
/// maps it to a raw string or boolean prop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrVal {
    /// `key="<html-escaped>"` in HTML; raw string in JS.
    Esc(String),
    /// `key="<percent-encoded>"` in HTML (href/src/id); raw string in JS.
    Url(String),
    /// `key="<verbatim>"` in HTML (already-safe: class/align/number); string in JS.
    Lit(String),
    /// Valueless boolean attribute (`data-footnotes`); `true` in JS.
    Flag,
    /// Empty-valued attribute (`disabled=""`); `true` in JS.
    Empty,
}

/// Ordered attribute set. Insertion order is preserved to match the
/// HTML adapter's byte output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Props(Vec<(&'static str, AttrVal)>);

impl Props {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn set(&mut self, key: &'static str, val: AttrVal) {
        self.0.push((key, val));
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &(&'static str, AttrVal)> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A node in the framework-neutral render tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VNode {
    /// A text node.
    Text { text: String },
    /// An element with structured children.
    Element {
        tag: &'static str,
        props: Props,
        children: Vec<VNode>,
        /// Emit `\n` after the open tag (HTML only).
        nl_open: bool,
        /// Emit `\n` after the close tag (HTML only).
        nl_close: bool,
        /// Self-closing void element (`<hr />`).
        void: bool,
    },
    /// Pre-rendered raw HTML (diagram SVG, math SVG, highlighted code,
    /// filtered raw HTML). `tag: None` is a bare fragment of HTML; a
    /// JS frontend wraps it in a `dangerouslySetInnerHTML` host element.
    Raw {
        tag: Option<&'static str>,
        props: Props,
        html: String,
        /// Emit `\n` after the node (HTML only).
        nl_close: bool,
    },
    /// A transparent grouping (renders only its children). Maps to a JS
    /// fragment; emits nothing of its own in HTML.
    Fragment { children: Vec<VNode> },
}

impl VNode {
    fn text(s: impl Into<String>) -> Self {
        VNode::Text { text: s.into() }
    }
}

/// Builder for `VNode::Element`.
struct El {
    tag: &'static str,
    props: Props,
    children: Vec<VNode>,
    nl_open: bool,
    nl_close: bool,
    void: bool,
}

impl El {
    fn new(tag: &'static str) -> Self {
        Self {
            tag,
            props: Props::new(),
            children: Vec::new(),
            nl_open: false,
            nl_close: false,
            void: false,
        }
    }

    fn attr(mut self, key: &'static str, val: AttrVal) -> Self {
        self.props.set(key, val);
        self
    }

    fn child(mut self, c: VNode) -> Self {
        self.children.push(c);
        self
    }

    fn kids(mut self, c: Vec<VNode>) -> Self {
        self.children = c;
        self
    }

    fn nlo(mut self) -> Self {
        self.nl_open = true;
        self
    }

    fn nlc(mut self) -> Self {
        self.nl_close = true;
        self
    }

    fn voided(mut self) -> Self {
        self.void = true;
        self
    }

    fn done(self) -> VNode {
        VNode::Element {
            tag: self.tag,
            props: self.props,
            children: self.children,
            nl_open: self.nl_open,
            nl_close: self.nl_close,
            void: self.void,
        }
    }
}

// ----- Errors and custom-renderer trait ----------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdomError {
    UnsupportedCustomNode,
    Sink(String),
}

impl fmt::Display for VdomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCustomNode => f.write_str("custom node renderer not configured"),
            Self::Sink(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for VdomError {}

pub trait RenderVdomCustom<Ext> {
    fn render_custom(&self, payload: &Ext, children: &[Node<Ext>]) -> Result<VNode, VdomError>;
}

impl<Ext> RenderVdomCustom<Ext> for () {
    fn render_custom(&self, _payload: &Ext, _children: &[Node<Ext>]) -> Result<VNode, VdomError> {
        Err(VdomError::UnsupportedCustomNode)
    }
}

// ----- Adapter -----------------------------------------------------

pub struct VdomAdapter<R = ()> {
    custom: R,
    html_filter: crate::HtmlFilter,
    allow_data_image_urls: bool,
    #[cfg(feature = "highlight")]
    highlighter: core::cell::RefCell<Option<Box<dyn crate::highlight::SpanHighlighter>>>,
    #[cfg(feature = "pikchr")]
    pikchr: bool,
    #[cfg(feature = "graphviz")]
    graphviz: bool,
    #[cfg(feature = "typst")]
    typst: bool,
}

impl Default for VdomAdapter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl VdomAdapter<()> {
    pub fn new() -> Self {
        Self {
            custom: (),
            html_filter: crate::HtmlFilter::safe(),
            allow_data_image_urls: true,
            #[cfg(feature = "highlight")]
            highlighter: core::cell::RefCell::new(None),
            #[cfg(feature = "pikchr")]
            pikchr: false,
            #[cfg(feature = "graphviz")]
            graphviz: false,
            #[cfg(feature = "typst")]
            typst: false,
        }
    }

    pub fn with_custom_renderer<Custom>(custom: Custom) -> VdomAdapter<Custom> {
        VdomAdapter {
            custom,
            html_filter: crate::HtmlFilter::safe(),
            allow_data_image_urls: true,
            #[cfg(feature = "highlight")]
            highlighter: core::cell::RefCell::new(None),
            #[cfg(feature = "pikchr")]
            pikchr: false,
            #[cfg(feature = "graphviz")]
            graphviz: false,
            #[cfg(feature = "typst")]
            typst: false,
        }
    }
}

impl<R> VdomAdapter<R> {
    pub fn with_html_filter(mut self, filter: crate::HtmlFilter) -> Self {
        self.html_filter = filter;
        self
    }

    pub fn without_data_image_urls(mut self) -> Self {
        self.allow_data_image_urls = false;
        self
    }
}

#[cfg(feature = "highlight")]
impl<R> VdomAdapter<R> {
    pub fn with_highlighter<H>(mut self, highlighter: H) -> Self
    where
        H: arborium::advanced::HighlightSpans + 'static,
    {
        self.highlighter = core::cell::RefCell::new(Some(Box::new(highlighter)));
        self
    }
}

#[cfg(feature = "pikchr")]
impl<R> VdomAdapter<R> {
    pub fn with_pikchr(mut self) -> Self {
        self.pikchr = true;
        self
    }
}

#[cfg(feature = "typst")]
impl<R> VdomAdapter<R> {
    pub fn with_typst(mut self) -> Self {
        self.typst = true;
        self
    }
}

#[cfg(feature = "graphviz")]
impl<R> VdomAdapter<R> {
    pub fn with_graphviz(mut self) -> Self {
        self.graphviz = true;
        self
    }
}

impl<R> VdomAdapter<R> {
    pub fn render<Ext>(&self, item: StreamItem<Node<Ext>>) -> Result<StreamItem<VNode>, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        match item {
            StreamItem::Upsert(mut emission) => {
                if emission.provisional {
                    crate::clear_provisional_diagram_info(&mut emission.node);
                }
                Ok(StreamItem::Upsert(Emission {
                    id: emission.id,
                    provisional: emission.provisional,
                    node: self.render_node(&emission.node)?,
                }))
            }
            StreamItem::Delete { id } => Ok(StreamItem::Delete { id }),
        }
    }

    pub fn render_node<Ext>(&self, node: &Node<Ext>) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        self.block(node)
    }

    // ----- code-block sub-dispatch (returns Some when it claimed it) -----

    #[cfg(feature = "highlight")]
    fn try_highlight(&self, info: Option<&str>, literal: &str) -> Option<VNode> {
        let lang = info
            .and_then(|s| s.split_whitespace().next())
            .filter(|s| !s.is_empty())?;
        let mut borrow = self.highlighter.borrow_mut();
        let hl = borrow.as_mut()?;
        let spans = hl.spans(lang, literal)?;
        let html = arborium::advanced::spans_to_html(
            literal,
            spans,
            &arborium::advanced::HtmlFormat::default(),
        );
        let code = El::new("code")
            .attr("class", AttrVal::Esc(format!("language-{lang}")))
            .child(VNode::Raw {
                tag: None,
                props: Props::new(),
                html,
                nl_close: false,
            })
            .done();
        Some(El::new("pre").child(code).nlc().done())
    }

    #[cfg(not(feature = "highlight"))]
    fn try_highlight(&self, _info: Option<&str>, _literal: &str) -> Option<VNode> {
        None
    }

    #[cfg(feature = "pikchr")]
    fn try_pikchr(&self, info: Option<&str>, literal: &str) -> Option<VNode> {
        let lang = info.and_then(|s| s.split_whitespace().next());
        if !self.pikchr || lang != Some("pikchr") {
            return None;
        }
        crate::render_pikchr_to_svg(literal).map(diagram_raw)
    }

    #[cfg(not(feature = "pikchr"))]
    fn try_pikchr(&self, _info: Option<&str>, _literal: &str) -> Option<VNode> {
        None
    }

    #[cfg(feature = "graphviz")]
    fn try_graphviz(&self, info: Option<&str>, literal: &str) -> Option<VNode> {
        let lang = info.and_then(|s| s.split_whitespace().next());
        if !self.graphviz || !matches!(lang, Some("dot") | Some("graphviz")) {
            return None;
        }
        crate::render_graphviz_to_svg(literal).map(diagram_raw)
    }

    #[cfg(not(feature = "graphviz"))]
    fn try_graphviz(&self, _info: Option<&str>, _literal: &str) -> Option<VNode> {
        None
    }

    #[cfg(feature = "typst")]
    fn try_typst(&self, info: Option<&str>, literal: &str) -> Option<VNode> {
        let lang = info.and_then(|s| s.split_whitespace().next());
        if !self.typst || lang != Some("typst") {
            return None;
        }
        crate::render_typst_to_svg(literal).map(diagram_raw)
    }

    #[cfg(not(feature = "typst"))]
    fn try_typst(&self, _info: Option<&str>, _literal: &str) -> Option<VNode> {
        None
    }

    fn code_block(&self, info: Option<&str>, literal: &str) -> VNode {
        let lang = info
            .and_then(|s| s.split_whitespace().next())
            .filter(|s| !s.is_empty());
        if matches!(lang, Some("ansi")) {
            return ansi_block(literal);
        }
        if matches!(lang, Some("terminal")) {
            return terminal_block(info, literal);
        }
        if let Some(v) = self
            .try_pikchr(info, literal)
            .or_else(|| self.try_graphviz(info, literal))
            .or_else(|| self.try_typst(info, literal))
            .or_else(|| self.try_highlight(info, literal))
        {
            return v;
        }
        let code = match lang {
            Some(lang) => El::new("code")
                .attr("class", AttrVal::Esc(format!("language-{lang}")))
                .child(VNode::text(literal)),
            None => El::new("code").child(VNode::text(literal)),
        };
        El::new("pre").child(code.done()).nlc().done()
    }

    // ----- block-level builder -----

    fn block<Ext>(&self, node: &Node<Ext>) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        let v = match node {
            Node::Document { children, .. } => VNode::Fragment {
                children: self.blocks(children)?,
            },
            Node::FootnoteSection { children, .. } => {
                if children.is_empty() {
                    VNode::Fragment {
                        children: Vec::new(),
                    }
                } else {
                    let ol = El::new("ol")
                        .kids(self.blocks(children)?)
                        .nlo()
                        .nlc()
                        .done();
                    El::new("section")
                        .attr("class", AttrVal::Lit("footnotes".into()))
                        .attr("data-footnotes", AttrVal::Flag)
                        .child(ol)
                        .nlo()
                        .nlc()
                        .done()
                }
            }
            Node::FootnoteDefinition {
                label,
                number,
                backref_count,
                children,
                ..
            } => self.footnote_definition(label, *number, *backref_count, children)?,
            Node::Paragraph { children, .. } => {
                El::new("p").kids(self.inlines(children)?).nlc().done()
            }
            Node::Heading {
                level, children, ..
            } => El::new(heading_tag(*level))
                .kids(self.inlines(children)?)
                .nlc()
                .done(),
            Node::BlockQuote { children, .. } => El::new("blockquote")
                .kids(self.blocks(children)?)
                .nlo()
                .nlc()
                .done(),
            Node::List {
                ordered,
                start,
                tight,
                children,
                ..
            } => self.list(*ordered, *start, tight.unwrap_or(true), children)?,
            Node::Item { children, .. } => {
                // Standalone item (not iterated as a list child).
                let mut li = El::new("li");
                if children.len() == 1 && matches!(children[0], Node::Paragraph { .. }) {
                    if let Node::Paragraph { children: inl, .. } = &children[0] {
                        li = li.kids(self.inlines(inl)?);
                    }
                } else {
                    li = li.child(VNode::text("\n"));
                    for child in children {
                        li = li.child(self.block(child)?);
                    }
                }
                li.nlc().done()
            }
            Node::TaskItem {
                checked, children, ..
            } => {
                let mut li = El::new("li")
                    .child(task_input(*checked))
                    .child(VNode::text(" "));
                if children.len() == 1 && matches!(children[0], Node::Paragraph { .. }) {
                    if let Node::Paragraph { children: inl, .. } = &children[0] {
                        for c in self.inlines(inl)? {
                            li = li.child(c);
                        }
                    }
                } else {
                    li = li.child(VNode::text("\n"));
                    for child in children {
                        li = li.child(self.block(child)?);
                    }
                }
                li.nlc().done()
            }
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => self.table(alignments, header, rows)?,
            Node::TableRow { .. } | Node::TableCell { .. } => self.table_row(node, &[])?,
            Node::CodeBlock { info, literal, .. } => self.code_block(info.as_deref(), literal),
            Node::MathBlock { math, .. } => math_block(math),
            Node::Html { html, .. } => VNode::Raw {
                tag: None,
                props: Props::new(),
                html: self.html_filter.filter(html),
                nl_close: !html.ends_with('\n'),
            },
            Node::ThematicBreak { .. } => El::new("hr").voided().nlc().done(),
            Node::Custom {
                payload, children, ..
            } => self.custom.render_custom(payload, children)?,
            Node::Alert {
                kind,
                title,
                children,
                ..
            } => {
                let (slug, default_title) = alert_slug(*kind);
                let shown = title.as_deref().unwrap_or(default_title);
                let title_p = El::new("p")
                    .attr("class", AttrVal::Lit("markdown-alert-title".into()))
                    .child(VNode::text(shown))
                    .nlc()
                    .done();
                let mut div = El::new("div")
                    .attr(
                        "class",
                        AttrVal::Lit(format!("markdown-alert markdown-alert-{slug}")),
                    )
                    .child(title_p);
                for child in children {
                    div = div.child(self.block(child)?);
                }
                div.nlo().nlc().done()
            }
            Node::DefinitionList { children, .. } => El::new("dl")
                .kids(self.blocks(children)?)
                .nlo()
                .nlc()
                .done(),
            Node::DefinitionTerm { children, .. } => {
                El::new("dt").kids(self.inlines(children)?).nlc().done()
            }
            Node::DefinitionDescription { children, .. } => {
                El::new("dd").kids(self.blocks(children)?).nlc().done()
            }
            // Inline nodes encountered in block position.
            Node::FootnoteRef { .. }
            | Node::MathInline { .. }
            | Node::Text { .. }
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
            | Node::HardBreak { .. } => self.inline(node)?,
        };
        Ok(v)
    }

    fn blocks<Ext>(&self, nodes: &[Node<Ext>]) -> Result<Vec<VNode>, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        nodes.iter().map(|n| self.block(n)).collect()
    }

    fn list<Ext>(
        &self,
        ordered: bool,
        start: Option<u64>,
        tight: bool,
        children: &[Node<Ext>],
    ) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        let mut el = if ordered {
            let e = El::new("ol");
            match start {
                Some(1) | None => e,
                Some(start) => e.attr("start", AttrVal::Lit(start.to_string())),
            }
        } else {
            El::new("ul")
        };
        let mut kids = Vec::new();
        for child in children {
            kids.push(self.list_child(child, tight)?);
        }
        el = el.kids(kids).nlo().nlc();
        Ok(el.done())
    }

    fn list_child<Ext>(&self, node: &Node<Ext>, tight: bool) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        match node {
            Node::Item { children, .. } => {
                let mut li = El::new("li");
                li = li.kids(self.li_children(children, tight)?);
                Ok(li.nlc().done())
            }
            Node::TaskItem {
                checked, children, ..
            } => {
                let mut kids = vec![task_input(*checked), VNode::text(" ")];
                kids.extend(self.li_children(children, tight)?);
                Ok(El::new("li").kids(kids).nlc().done())
            }
            other => self.block(other),
        }
    }

    /// Children of an `<li>` (between `<li…>` and `</li>`), excluding any
    /// leading task-checkbox. Mirrors the HTML adapter's tight/loose rules.
    fn li_children<Ext>(&self, children: &[Node<Ext>], tight: bool) -> Result<Vec<VNode>, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        let mut out = Vec::new();
        if children.is_empty() {
            return Ok(out);
        }
        if tight {
            if !matches!(children.first(), Some(Node::Paragraph { .. })) {
                out.push(VNode::text("\n"));
            }
            for (i, child) in children.iter().enumerate() {
                if let Node::Paragraph { children: inl, .. } = child {
                    out.extend(self.inlines(inl)?);
                    if i + 1 < children.len() && matches!(children[i + 1], Node::Paragraph { .. }) {
                        out.push(VNode::text("\n"));
                    }
                } else {
                    out.push(self.block(child)?);
                }
            }
        } else {
            out.push(VNode::text("\n"));
            for child in children {
                out.push(self.block(child)?);
            }
        }
        Ok(out)
    }

    fn table<Ext>(
        &self,
        alignments: &[Option<TableAlignment>],
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
    ) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        let mut head_rows = Vec::new();
        for row in header {
            head_rows.push(self.table_row(row, alignments)?);
        }
        let thead = El::new("thead").kids(head_rows).nlo().nlc().done();
        let mut table = El::new("table").child(thead);
        if !rows.is_empty() {
            let mut body_rows = Vec::new();
            for row in rows {
                body_rows.push(self.table_row(row, alignments)?);
            }
            let tbody = El::new("tbody").kids(body_rows).nlo().nlc().done();
            table = table.child(tbody);
        }
        Ok(table.nlo().nlc().done())
    }

    fn table_row<Ext>(
        &self,
        node: &Node<Ext>,
        alignments: &[Option<TableAlignment>],
    ) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        let Node::TableRow { cells, .. } = node else {
            return Ok(VNode::Fragment {
                children: Vec::new(),
            });
        };
        let mut tr = El::new("tr");
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
            let align = alignment.or_else(|| alignments.get(index).copied().flatten());
            let mut td = El::new(tag);
            if let Some(align) = align {
                td = td.attr("align", AttrVal::Lit(align_str(align).into()));
            }
            td = td.kids(self.inlines(children)?);
            tr = tr.child(td.done());
        }
        Ok(tr.nlc().done())
    }

    fn footnote_definition<Ext>(
        &self,
        label: &str,
        number: u32,
        backref_count: u32,
        children: &[Node<Ext>],
    ) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        let mut li = El::new("li")
            .attr("id", AttrVal::Url(format!("fn-{label}")))
            .nlo();
        if children.is_empty() {
            li = li.kids(footnote_backrefs(label, number, backref_count));
            li = li.child(VNode::text("\n"));
        } else {
            let last_idx = children.len() - 1;
            let mut kids = Vec::new();
            for (idx, child) in children.iter().enumerate() {
                if idx == last_idx {
                    if let Node::Paragraph { children: inl, .. } = child {
                        let mut p_children = self.inlines(inl)?;
                        p_children.push(VNode::text(" "));
                        p_children.extend(footnote_backrefs(label, number, backref_count));
                        kids.push(El::new("p").kids(p_children).nlc().done());
                    } else {
                        kids.push(self.block(child)?);
                        kids.extend(footnote_backrefs(label, number, backref_count));
                        kids.push(VNode::text("\n"));
                    }
                } else {
                    kids.push(self.block(child)?);
                }
            }
            li = li.kids({
                // `nlo()` already emits the `\n` after `<li…>`, so keep kids as-is.
                kids
            });
        }
        Ok(li.nlc().done())
    }

    // ----- inline-level builder -----

    fn inlines<Ext>(&self, nodes: &[Node<Ext>]) -> Result<Vec<VNode>, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        nodes.iter().map(|n| self.inline(n)).collect()
    }

    fn inline<Ext>(&self, node: &Node<Ext>) -> Result<VNode, VdomError>
    where
        R: RenderVdomCustom<Ext>,
    {
        let v = match node {
            Node::Text { text, .. } => VNode::text(text.clone()),
            Node::FootnoteRef {
                label,
                number,
                ref_num,
                ..
            } => {
                let id = if *ref_num > 1 {
                    format!("fnref-{label}-{ref_num}")
                } else {
                    format!("fnref-{label}")
                };
                let a = El::new("a")
                    .attr("href", AttrVal::Url(format!("#fn-{label}")))
                    .attr("id", AttrVal::Url(id))
                    .attr("data-footnote-ref", AttrVal::Flag)
                    .child(VNode::text(number.to_string()))
                    .done();
                El::new("sup")
                    .attr("class", AttrVal::Lit("footnote-ref".into()))
                    .child(a)
                    .done()
            }
            Node::Emphasis { children, .. } => El::new("em").kids(self.inlines(children)?).done(),
            Node::Strong { children, .. } => El::new("strong").kids(self.inlines(children)?).done(),
            Node::Strikethrough { children, .. } => {
                El::new("del").kids(self.inlines(children)?).done()
            }
            Node::Subscript { children, .. } => El::new("sub").kids(self.inlines(children)?).done(),
            Node::Superscript { children, .. } => {
                El::new("sup").kids(self.inlines(children)?).done()
            }
            Node::Highlight { children, .. } => {
                El::new("mark").kids(self.inlines(children)?).done()
            }
            Node::Link {
                destination,
                title,
                children,
                ..
            } => {
                let href = if crate::url_safety::is_safe_url(destination) {
                    destination.clone()
                } else {
                    String::new()
                };
                let mut a = El::new("a").attr("href", AttrVal::Url(href));
                if let Some(title) = title {
                    a = a.attr("title", AttrVal::Esc(title.clone()));
                }
                a.kids(self.inlines(children)?).done()
            }
            Node::Image {
                destination,
                title,
                alt,
                ..
            } => {
                let url_ok = if self.allow_data_image_urls {
                    crate::url_safety::is_safe_image_url(destination)
                } else {
                    crate::url_safety::is_safe_url(destination)
                };
                let src = if url_ok {
                    destination.clone()
                } else {
                    String::new()
                };
                let mut img = El::new("img")
                    .attr("src", AttrVal::Url(src))
                    .attr("alt", AttrVal::Esc(alt.clone()));
                if let Some(title) = title {
                    img = img.attr("title", AttrVal::Esc(title.clone()));
                }
                img.voided().done()
            }
            Node::CodeSpan { code, .. } => El::new("code").child(VNode::text(code.clone())).done(),
            Node::MathInline { math, .. } => math_inline(math, false),
            Node::MathBlock { math, .. } => math_inline(math, true),
            Node::Html { html, .. } => VNode::Raw {
                tag: None,
                props: Props::new(),
                html: self.html_filter.filter(html),
                nl_close: false,
            },
            Node::SoftBreak { .. } => VNode::text("\n"),
            Node::HardBreak { .. } => El::new("br").voided().nlc().done(),
            Node::Custom {
                payload, children, ..
            } => self.custom.render_custom(payload, children)?,
            // Block nodes encountered in inline position.
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
            | Node::Document { .. } => self.block(node)?,
        };
        Ok(v)
    }
}

// ----- free helpers ------------------------------------------------

fn heading_tag(level: u8) -> &'static str {
    match level {
        1 => "h1",
        2 => "h2",
        3 => "h3",
        4 => "h4",
        5 => "h5",
        _ => "h6",
    }
}

fn align_str(align: TableAlignment) -> &'static str {
    match align {
        TableAlignment::Left => "left",
        TableAlignment::Center => "center",
        TableAlignment::Right => "right",
    }
}

fn alert_slug(kind: AlertKind) -> (&'static str, &'static str) {
    match kind {
        AlertKind::Note => ("note", "Note"),
        AlertKind::Tip => ("tip", "Tip"),
        AlertKind::Important => ("important", "Important"),
        AlertKind::Warning => ("warning", "Warning"),
        AlertKind::Caution => ("caution", "Caution"),
    }
}

fn task_input(checked: bool) -> VNode {
    let mut input = El::new("input")
        .attr("type", AttrVal::Lit("checkbox".into()))
        .attr("disabled", AttrVal::Empty);
    if checked {
        input = input.attr("checked", AttrVal::Empty);
    }
    input.voided().done()
}

/// Wrap a finalized (sanitized + outlined) diagram SVG as a bare raw
/// node, matching the HTML adapter's "emit SVG verbatim, ensure one
/// trailing newline" behavior.
#[cfg(any(feature = "pikchr", feature = "graphviz", feature = "typst"))]
fn diagram_raw(final_svg: String) -> VNode {
    let nl_close = !final_svg.ends_with('\n');
    VNode::Raw {
        tag: None,
        props: Props::new(),
        html: final_svg,
        nl_close,
    }
}

fn math_block(math: &str) -> VNode {
    match mathml_core::latex_to_svg(math, 22.0) {
        Ok(svg) => VNode::Raw {
            tag: Some("div"),
            props: prop_class("math math-display"),
            html: svg,
            nl_close: true,
        },
        Err(_) => El::new("pre")
            .child(
                El::new("code")
                    .attr(
                        "class",
                        AttrVal::Lit("language-math math-display math-error".into()),
                    )
                    .child(VNode::text(math))
                    .done(),
            )
            .nlc()
            .done(),
    }
}

fn math_inline(math: &str, display: bool) -> VNode {
    let size = if display { 22.0 } else { 16.0 };
    let (ok_class, err_class) = if display {
        ("math math-display", "language-math math-display math-error")
    } else {
        ("math math-inline", "language-math math-inline math-error")
    };
    match mathml_core::latex_to_svg(math, size) {
        Ok(svg) => VNode::Raw {
            tag: Some("span"),
            props: prop_class(ok_class),
            html: svg,
            nl_close: false,
        },
        Err(_) => El::new("code")
            .attr("class", AttrVal::Lit(err_class.into()))
            .child(VNode::text(math))
            .done(),
    }
}

fn prop_class(class: &str) -> Props {
    let mut p = Props::new();
    p.set("class", AttrVal::Lit(class.to_string()));
    p
}

fn footnote_backrefs(label: &str, number: u32, count: u32) -> Vec<VNode> {
    let count = count.max(1);
    let mut out = Vec::new();
    for i in 1..=count {
        if i > 1 {
            out.push(VNode::text(" "));
        }
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
        let mut a = El::new("a")
            .attr("href", AttrVal::Url(href))
            .attr("class", AttrVal::Lit("footnote-backref".into()))
            .attr("data-footnote-backref", AttrVal::Flag)
            .attr("data-footnote-backref-idx", AttrVal::Lit(idx))
            .attr("aria-label", AttrVal::Lit(aria))
            .child(VNode::text("↩"));
        if i > 1 {
            a = a.child(
                El::new("sup")
                    .attr("class", AttrVal::Lit("footnote-ref".into()))
                    .child(VNode::text(i.to_string()))
                    .done(),
            );
        }
        out.push(a.done());
    }
    out
}

/// Determine whether any child is a block-level node — used by the
/// HTML back-compat bridge to decide a trailing newline after a custom
/// node, matching the previous `HtmlAdapter` behavior.
pub(crate) fn is_block_like<Ext>(children: &[Node<Ext>]) -> bool {
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

// ----- ANSI / terminal two-pane blocks -----------------------------

#[cfg(feature = "ansi")]
fn ansi_block(literal: &str) -> VNode {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    let mut html = String::new();
    html.push_str("<div class=\"ansi-block\">");
    html.push_str(&format!(
        "<input type=\"checkbox\" id=\"ansi-src-{id}\" class=\"ansi-toggle-input\" aria-hidden=\"true\" />"
    ));
    html.push_str(&format!(
        "<label for=\"ansi-src-{id}\" class=\"ansi-toggle\"><span class=\"ansi-toggle-show\">show source</span><span class=\"ansi-toggle-hide\">show rendered</span></label>"
    ));
    html.push_str("<pre class=\"ansi-rendered\"><code class=\"language-ansi\">");
    html.push_str(&crate::ansi_to_html::ansi_to_html(literal));
    html.push_str("</code></pre>");
    html.push_str("<pre class=\"ansi-source\"><code class=\"language-ansi-source\">");
    html.push_str(&crate::ansi_to_html::ansi_to_annotated_source(literal));
    html.push_str("</code></pre>");
    html.push_str("</div>\n");
    VNode::Raw {
        tag: None,
        props: Props::new(),
        html,
        nl_close: false,
    }
}

#[cfg(not(feature = "ansi"))]
fn ansi_block(literal: &str) -> VNode {
    let mut html = String::from("<pre><code class=\"language-ansi\">");
    crate::adapter::escape::escape_html(literal, &mut html);
    html.push_str("</code></pre>\n");
    VNode::Raw {
        tag: None,
        props: Props::new(),
        html,
        nl_close: false,
    }
}

#[cfg(feature = "terminal")]
fn terminal_block(info: Option<&str>, literal: &str) -> VNode {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    let (cols, rows) = parse_terminal_geometry(info);

    let mut html = String::new();
    html.push_str("<div class=\"terminal-block\">");
    html.push_str(&format!(
        "<input type=\"checkbox\" id=\"terminal-src-{id}\" class=\"terminal-toggle-input\" aria-hidden=\"true\" />"
    ));
    html.push_str(&format!(
        "<label for=\"terminal-src-{id}\" class=\"terminal-toggle\"><span class=\"terminal-toggle-show\">show source</span><span class=\"terminal-toggle-hide\">show rendered</span></label>"
    ));
    html.push_str("<pre class=\"terminal-rendered\"><code class=\"language-terminal\">");
    html.push_str(&crate::terminal::ansi_snapshot_to_html(literal, cols, rows));
    html.push_str("</code></pre>");
    html.push_str("<pre class=\"terminal-source\"><code class=\"language-terminal-source\">");
    html.push_str(&crate::ansi_to_html::ansi_to_annotated_source(literal));
    html.push_str("</code></pre>");
    html.push_str("</div>\n");
    VNode::Raw {
        tag: None,
        props: Props::new(),
        html,
        nl_close: false,
    }
}

#[cfg(not(feature = "terminal"))]
fn terminal_block(_info: Option<&str>, literal: &str) -> VNode {
    let mut html = String::from("<pre><code class=\"language-terminal\">");
    crate::adapter::escape::escape_html(literal, &mut html);
    html.push_str("</code></pre>\n");
    VNode::Raw {
        tag: None,
        props: Props::new(),
        html,
        nl_close: false,
    }
}

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

// ----- streaming sink ----------------------------------------------

pub struct VdomSink<S, R = ()> {
    inner: S,
    adapter: VdomAdapter<R>,
}

impl<S> VdomSink<S, ()> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            adapter: VdomAdapter::new(),
        }
    }
}

impl<S, R> VdomSink<S, R> {
    pub fn with_adapter(inner: S, adapter: VdomAdapter<R>) -> Self {
        Self { inner, adapter }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, R, Ext> Sink<Node<Ext>> for VdomSink<S, R>
where
    S: Sink<VNode>,
    S::Error: fmt::Display,
    R: RenderVdomCustom<Ext>,
{
    type Error = VdomError;

    fn emit(&mut self, item: StreamItem<Node<Ext>>) -> Result<(), Self::Error> {
        let rendered = self.adapter.render(item)?;
        self.inner
            .emit(rendered)
            .map_err(|error| VdomError::Sink(error.to_string()))
    }
}

// ----- serde (JS frontend) -----------------------------------------

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::VdomAdapter;
    use crate::node::{Node, SourceSpan};

    fn sp() -> SourceSpan {
        SourceSpan::default()
    }

    fn text(s: &str) -> Node<()> {
        Node::Text {
            text: s.to_owned(),
            source: sp(),
        }
    }

    fn json(node: &Node<()>) -> String {
        let v = VdomAdapter::new().render_node(node).unwrap();
        serde_json::to_string(&v).unwrap()
    }

    #[test]
    fn paragraph_serializes_to_element_with_text_child() {
        let n = Node::Paragraph {
            children: vec![text("hi")],
            source: sp(),
        };
        assert_eq!(
            json(&n),
            r#"{"tag":"p","props":{},"children":[{"text":"hi"}]}"#
        );
    }

    #[test]
    fn tight_list_in_blockquote_nests_elements() {
        let n = Node::BlockQuote {
            children: vec![Node::List {
                ordered: false,
                start: None,
                tight: Some(true),
                children: vec![Node::Item {
                    children: vec![Node::Paragraph {
                        children: vec![text("item")],
                        source: sp(),
                    }],
                    source: sp(),
                }],
                source: sp(),
            }],
            source: sp(),
        };
        assert_eq!(
            json(&n),
            r#"{"tag":"blockquote","props":{},"children":[{"tag":"ul","props":{},"children":[{"tag":"li","props":{},"children":[{"text":"item"}]}]}]}"#
        );
    }

    #[test]
    fn code_block_serializes_to_pre_code() {
        let n = Node::CodeBlock {
            info: Some("rust".to_owned()),
            literal: "x".to_owned(),
            source: sp(),
        };
        assert_eq!(
            json(&n),
            r#"{"tag":"pre","props":{},"children":[{"tag":"code","props":{"class":"language-rust"},"children":[{"text":"x"}]}]}"#
        );
    }

    #[test]
    fn task_item_uses_boolean_props_and_void_input() {
        let n = Node::List {
            ordered: false,
            start: None,
            tight: Some(true),
            children: vec![Node::TaskItem {
                checked: true,
                children: vec![Node::Paragraph {
                    children: vec![text("done")],
                    source: sp(),
                }],
                source: sp(),
            }],
            source: sp(),
        };
        let out = json(&n);
        assert!(
            out.contains(r#"{"tag":"input","props":{"type":"checkbox","disabled":true,"checked":true},"children":[]}"#),
            "got: {out}"
        );
    }

    #[test]
    fn javascript_link_destination_is_emptied() {
        let n = Node::Paragraph {
            children: vec![Node::Link {
                destination: "javascript:alert(1)".to_owned(),
                title: None,
                children: vec![text("x")],
                source: sp(),
            }],
            source: sp(),
        };
        let out = json(&n);
        assert!(out.contains(r#""href":"""#), "got: {out}");
        assert!(!out.contains("javascript"), "got: {out}");
    }

    #[test]
    fn inline_math_becomes_raw_span() {
        let n = Node::Paragraph {
            children: vec![Node::MathInline {
                math: "x".to_owned(),
                source: sp(),
            }],
            source: sp(),
        };
        let out = json(&n);
        // mathml-core renders to SVG; assert the structural wrapper, not the SVG body.
        assert!(out.contains(r#""tag":"span""#), "got: {out}");
        assert!(out.contains("math math-inline"), "got: {out}");
        assert!(out.contains(r#""html":"#), "got: {out}");
    }
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::{AttrVal, Props, VNode};
    use serde::ser::{Serialize, SerializeMap, SerializeStruct, Serializer};

    impl Serialize for Props {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(self.0.len()))?;
            for (key, val) in &self.0 {
                match val {
                    AttrVal::Esc(s) | AttrVal::Url(s) | AttrVal::Lit(s) => {
                        map.serialize_entry(key, s)?;
                    }
                    AttrVal::Flag | AttrVal::Empty => {
                        map.serialize_entry(key, &true)?;
                    }
                }
            }
            map.end()
        }
    }

    impl Serialize for VNode {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                VNode::Text { text } => {
                    let mut s = serializer.serialize_struct("VNode", 1)?;
                    s.serialize_field("text", text)?;
                    s.end()
                }
                VNode::Element {
                    tag,
                    props,
                    children,
                    ..
                } => {
                    let mut s = serializer.serialize_struct("VNode", 3)?;
                    s.serialize_field("tag", tag)?;
                    s.serialize_field("props", props)?;
                    s.serialize_field("children", children)?;
                    s.end()
                }
                VNode::Raw {
                    tag, props, html, ..
                } => {
                    let mut s = serializer.serialize_struct("VNode", 3)?;
                    s.serialize_field("tag", tag)?;
                    s.serialize_field("props", props)?;
                    s.serialize_field("html", html)?;
                    s.end()
                }
                VNode::Fragment { children } => {
                    let mut s = serializer.serialize_struct("VNode", 1)?;
                    s.serialize_field("children", children)?;
                    s.end()
                }
            }
        }
    }
}
