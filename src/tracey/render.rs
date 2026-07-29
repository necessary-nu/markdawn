//! Sync render pipeline that mirrors marq's `render(md, opts) -> Document` API.

use std::fmt;
use std::sync::Arc;

use crate::adapter::{HtmlAdapter, HtmlError};
use crate::editor::EditableDocument;
use crate::node::{Node, SourceSpan as MdSpan, StreamItem};
use crate::parser::Parser;

use super::extension::{ReqPayload, TraceyExt, extract_reqs};
use super::frontmatter::{Frontmatter, FrontmatterError, parse_frontmatter};
use super::handler::{HandlerError, InlineCodeHandler, ReqHandler};
use super::heading::{Heading, slugify};
use super::req::{InlineCodeSpan, ReqDefinition, SourceSpan};

/// A rendered markdown document.
// [spec:mw:def:traceyext.render.document]
#[derive(Debug, Clone)]
pub struct Document {
    pub html: String,
    pub frontmatter: Option<Frontmatter>,
    pub headings: Vec<Heading>,
    pub reqs: Vec<ReqDefinition>,
    pub elements: Vec<DocElement>,
    pub head_injections: Vec<String>,
    pub inline_code_spans: Vec<InlineCodeSpan>,
}

/// A document element in source order. Useful for outline / TOC building.
#[derive(Debug, Clone)]
pub enum DocElement {
    Heading(Heading),
    Req(ReqDefinition),
    Paragraph(Paragraph),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paragraph {
    pub line: usize,
    pub offset: usize,
}

/// Options for rendering.
// [spec:mw:def:traceyext.render.document]
#[derive(Default, Clone)]
pub struct RenderOptions {
    pub source_path: Option<String>,
    req_handler: Option<Arc<dyn ReqHandler>>,
    inline_code_handler: Option<Arc<dyn InlineCodeHandler>>,
}

impl RenderOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_source_path(mut self, path: &str) -> Self {
        self.source_path = Some(path.to_string());
        self
    }

    pub fn with_req_handler<H: ReqHandler + 'static>(mut self, handler: H) -> Self {
        self.req_handler = Some(Arc::new(handler));
        self
    }

    pub fn with_inline_code_handler<H: InlineCodeHandler + 'static>(mut self, handler: H) -> Self {
        self.inline_code_handler = Some(Arc::new(handler));
        self
    }
}

#[derive(Debug)]
pub enum RenderError {
    Frontmatter(FrontmatterError),
    Parse(String),
    Html(HtmlError),
    Handler(HandlerError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frontmatter(e) => write!(f, "{e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::Html(e) => write!(f, "{e}"),
            Self::Handler(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<FrontmatterError> for RenderError {
    fn from(e: FrontmatterError) -> Self {
        Self::Frontmatter(e)
    }
}

impl From<HtmlError> for RenderError {
    fn from(e: HtmlError) -> Self {
        Self::Html(e)
    }
}

impl From<HandlerError> for RenderError {
    fn from(e: HandlerError) -> Self {
        Self::Handler(e)
    }
}

/// Render markdown to HTML and extract structured tracey data.
// [spec:mw:sem:traceyext.render.pipeline]
pub fn render(markdown: &str, opts: &RenderOptions) -> Result<Document, RenderError> {
    let (frontmatter, body) = parse_frontmatter(markdown)?;
    let frontmatter = if !frontmatter_is_default(&frontmatter) {
        Some(frontmatter)
    } else {
        None
    };
    let base_offset = markdown.len() - body.len();

    // Parse body with markdawn (GFM, math, alerts, highlights, definition
    // lists). EditableDocument is used (over the raw streaming parser)
    // because it populates `Node::source` byte ranges via `parse_full_at`,
    // which the streaming `push_str` path leaves zeroed.
    let parser = Parser::<TraceyExt>::builder().with_all().build();
    let mut doc = EditableDocument::new(parser);
    doc.set_source(body)
        .map_err(|e| RenderError::Parse(format!("{e:?}")))?;
    let mut nodes: Vec<Node<TraceyExt>> = doc.blocks().iter().map(|b| b.node.clone()).collect();

    // Walk the tree and convert paragraph/blockquote markers into Custom nodes.
    extract_reqs(&mut nodes, markdown, base_offset);

    // Apply inline-code rewriting if configured (replace CodeSpan → Html).
    if let Some(handler) = opts.inline_code_handler.as_ref() {
        for node in nodes.iter_mut() {
            rewrite_inline_code(node, handler.as_ref());
        }
    }

    // Collect headings, reqs, paragraphs, and inline code spans.
    let mut sink = MetadataSink::default();
    for node in &nodes {
        collect_metadata(node, markdown, base_offset, &mut sink);
    }
    let MetadataSink {
        headings,
        reqs,
        elements,
        inline_code_spans,
        ..
    } = sink;

    // Pre-render each Req Custom node's body HTML into a string so we can drop
    // it into the document via Node::Html. We use a fresh HtmlAdapter (no
    // custom impl) for inner rendering.
    let inner_adapter: HtmlAdapter<()> = build_html_adapter();

    // Replace Custom nodes with rendered Html nodes carrying start + body + end.
    let req_handler = opts
        .req_handler
        .clone()
        .unwrap_or_else(|| Arc::new(super::handler::DefaultReqHandler));
    let mut req_iter = reqs.iter();
    for node in nodes.iter_mut() {
        replace_reqs_with_html(node, &inner_adapter, req_handler.as_ref(), &mut req_iter)?;
    }

    // Render the (now Custom-free) tree to HTML.
    let outer_adapter: HtmlAdapter<()> = build_html_adapter();
    let mut html = String::new();
    for node in &nodes {
        let item = StreamItem::Upsert(crate::node::Emission {
            id: 0,
            provisional: false,
            node: node.clone(),
        });
        match outer_adapter.render(item)? {
            StreamItem::Upsert(emission) => html.push_str(&emission.node),
            StreamItem::Delete { .. } => {}
        }
    }

    // Now backfill `html` field on each ReqDefinition by re-rendering its
    // children (we kept reqs in document order; collect children HTML once).
    // The rendered `html` was already used to build the wrapper; for the
    // Document.reqs vector, we want body-only HTML. Compute it now.
    // To do so we need access to the (already replaced) Custom children. Walk
    // the tree once more to find Html nodes that came from req replacements;
    // but easier: we'll have stored the body HTML during replace step.
    // (See: req body html is captured in `reqs` Vec via pre_render below.)

    Ok(Document {
        html,
        frontmatter,
        headings,
        reqs,
        elements,
        head_injections: Vec::new(),
        inline_code_spans,
    })
}

fn frontmatter_is_default(fm: &Frontmatter) -> bool {
    fm.title.is_empty() && fm.weight == 0 && fm.description.is_none() && fm.template.is_none()
}

fn build_html_adapter() -> HtmlAdapter<()> {
    // Tracey input is project-owner spec markdown (treated as trusted, like
    // Cargo.toml), and tracey injects its own ReqHandler-rendered HTML via
    // `Node::Html`. Sanitizing either path strips attributes/classes the
    // dashboard relies on, so the outer adapter passes HTML through.
    let adapter = HtmlAdapter::new().with_html_filter(crate::HtmlFilter::unrestricted());
    #[cfg(feature = "highlight")]
    let adapter = adapter.with_highlighter(crate::arborium::Highlighter::new());
    #[cfg(feature = "pikchr")]
    let adapter = adapter.with_pikchr();
    adapter
}

/// Accumulators threaded through [`collect_metadata`]'s recursive walk:
/// the document-order outputs plus the ancestor-heading stack that
/// drives hierarchical heading ids.
#[derive(Default)]
struct MetadataSink {
    headings: Vec<Heading>,
    reqs: Vec<ReqDefinition>,
    elements: Vec<DocElement>,
    inline_code_spans: Vec<InlineCodeSpan>,
    heading_stack: Vec<(u8, String)>,
}

// [spec:mw:req:traceyext.span.absolute-offsets]
// [spec:mw:req:traceyext.heading.slug]
fn collect_metadata(
    node: &Node<TraceyExt>,
    source: &str,
    base_offset: usize,
    sink: &mut MetadataSink,
) {
    match node {
        Node::Heading {
            level,
            children,
            source: span,
        } => {
            let title = inline_text(children);
            let line = byte_offset_to_line(source, span.start + base_offset);
            let slug = slugify(&title);

            // Pop any deeper-or-equal-level frames so the stack reflects
            // strictly-shallower ancestors of the current heading.
            while sink
                .heading_stack
                .last()
                .is_some_and(|(lvl, _)| *lvl >= *level)
            {
                sink.heading_stack.pop();
            }

            // Hierarchical id: ancestor slugs joined by `--`.
            let id = if sink.heading_stack.is_empty() {
                slug.clone()
            } else {
                let mut acc = String::new();
                for (_, parent) in sink.heading_stack.iter() {
                    acc.push_str(parent);
                    acc.push_str("--");
                }
                acc.push_str(&slug);
                acc
            };

            sink.heading_stack.push((*level, slug));

            let h = Heading {
                title,
                id,
                level: *level,
                line,
            };
            sink.headings.push(h.clone());
            sink.elements.push(DocElement::Heading(h));
        }
        Node::Custom {
            payload: TraceyExt::Req(p),
            children,
            source: span,
        } => {
            let raw = req_raw_text(source, span, base_offset, p);
            let body_html = render_children_to_html(children);
            // The payload's `line` field is left at 0 in extract_reqs;
            // compute it here from the absolute marker offset.
            let line = byte_offset_to_line(source, p.marker_offset);
            let req = ReqDefinition {
                id: p.id.clone(),
                kind: p.kind,
                anchor_id: p.anchor_id.clone(),
                marker_span: SourceSpan::new(p.marker_offset, p.marker_length),
                span: SourceSpan::new(
                    span.start + base_offset,
                    span.end.saturating_sub(span.start),
                ),
                line,
                raw,
                html: body_html,
            };
            sink.reqs.push(req.clone());
            sink.elements.push(DocElement::Req(req));
            // Walk children for inline code spans only (no nested reqs/headings expected).
            for child in children {
                collect_inline_code(child, source, base_offset, &mut sink.inline_code_spans);
            }
        }
        Node::Paragraph {
            source: span,
            children,
        } => {
            let line = byte_offset_to_line(source, span.start + base_offset);
            sink.elements.push(DocElement::Paragraph(Paragraph {
                line,
                offset: span.start + base_offset,
            }));
            for child in children {
                collect_inline_code(child, source, base_offset, &mut sink.inline_code_spans);
            }
        }
        _ => {
            // Recurse into containers.
            walk_block_children(node, |child| {
                collect_metadata(child, source, base_offset, sink);
            });
        }
    }
}

fn walk_block_children<Ext>(node: &Node<Ext>, mut f: impl FnMut(&Node<Ext>)) {
    match node {
        Node::Document { children, .. }
        | Node::BlockQuote { children, .. }
        | Node::List { children, .. }
        | Node::Item { children, .. }
        | Node::TaskItem { children, .. }
        | Node::FootnoteDefinition { children, .. }
        | Node::FootnoteSection { children, .. }
        | Node::Alert { children, .. }
        | Node::DefinitionList { children, .. }
        | Node::DefinitionTerm { children, .. }
        | Node::DefinitionDescription { children, .. } => {
            for child in children {
                f(child);
            }
        }
        Node::Table { header, rows, .. } => {
            for row in header {
                f(row);
            }
            for row in rows {
                f(row);
            }
        }
        _ => {}
    }
}

fn collect_inline_code<Ext>(
    node: &Node<Ext>,
    _source: &str,
    base_offset: usize,
    out: &mut Vec<InlineCodeSpan>,
) {
    match node {
        Node::CodeSpan { code, source } => {
            out.push(InlineCodeSpan {
                content: code.clone(),
                span: SourceSpan::new(
                    source.start + base_offset,
                    source.end.saturating_sub(source.start),
                ),
            });
        }
        Node::Emphasis { children, .. }
        | Node::Strong { children, .. }
        | Node::Strikethrough { children, .. }
        | Node::Subscript { children, .. }
        | Node::Superscript { children, .. }
        | Node::Highlight { children, .. }
        | Node::Link { children, .. } => {
            for child in children {
                collect_inline_code(child, _source, base_offset, out);
            }
        }
        Node::Paragraph { children, .. } => {
            for child in children {
                collect_inline_code(child, _source, base_offset, out);
            }
        }
        _ => {}
    }
}

fn inline_text<Ext>(children: &[Node<Ext>]) -> String {
    let mut out = String::new();
    for child in children {
        push_inline_text(child, &mut out);
    }
    out.trim().to_string()
}

fn push_inline_text<Ext>(node: &Node<Ext>, out: &mut String) {
    match node {
        Node::Text { text, .. } => out.push_str(text),
        Node::CodeSpan { code, .. } => out.push_str(code),
        Node::Emphasis { children, .. }
        | Node::Strong { children, .. }
        | Node::Strikethrough { children, .. }
        | Node::Subscript { children, .. }
        | Node::Superscript { children, .. }
        | Node::Highlight { children, .. }
        | Node::Link { children, .. } => {
            for child in children {
                push_inline_text(child, out);
            }
        }
        Node::SoftBreak { .. } | Node::HardBreak { .. } => out.push(' '),
        _ => {}
    }
}

fn byte_offset_to_line(source: &str, byte_offset: usize) -> usize {
    1 + source[..byte_offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
}

fn req_raw_text(source: &str, span: &MdSpan, base_offset: usize, p: &ReqPayload) -> String {
    let abs_start = span.start + base_offset;
    let abs_end = span.end + base_offset;
    let block = source.get(abs_start..abs_end).unwrap_or("");
    // Strip the marker and any leading whitespace after it.
    let marker_rel = p.marker_offset.saturating_sub(abs_start);
    let after_marker = block.get(marker_rel + p.marker_length..).unwrap_or(block);
    after_marker.trim_start_matches([' ', '\t']).to_string()
}

fn render_children_to_html(children: &[Node<TraceyExt>]) -> String {
    // Render each child as a top-level block via a fresh adapter. Children
    // shouldn't contain nested Custom nodes (no nested reqs).
    let adapter: HtmlAdapter<()> = build_html_adapter();
    let mut out = String::new();
    for child in children {
        // SAFETY for the cast: we only contain Custom payloads that we ourselves
        // produced; render_children_to_html is invoked on req body content
        // which by construction has no Custom nodes.
        let cloned: Node<()> = strip_custom(child);
        let item = StreamItem::Upsert(crate::node::Emission {
            id: 0,
            provisional: false,
            node: cloned,
        });
        if let Ok(StreamItem::Upsert(e)) = adapter.render(item) {
            out.push_str(&e.node);
        }
    }
    out
}

/// Convert `Node<TraceyExt>` → `Node<()>`. Custom nodes (which shouldn't appear
/// inside req bodies) are dropped (replaced with empty paragraphs).
fn strip_custom(node: &Node<TraceyExt>) -> Node<()> {
    match node {
        Node::Document { children, source } => Node::Document {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Paragraph { children, source } => Node::Paragraph {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Heading {
            level,
            children,
            source,
        } => Node::Heading {
            level: *level,
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::BlockQuote { children, source } => Node::BlockQuote {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::List {
            ordered,
            start,
            tight,
            children,
            source,
        } => Node::List {
            ordered: *ordered,
            start: *start,
            tight: *tight,
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Item { children, source } => Node::Item {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::TaskItem {
            checked,
            children,
            source,
        } => Node::TaskItem {
            checked: *checked,
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Table {
            alignments,
            header,
            rows,
            source,
        } => Node::Table {
            alignments: alignments.clone(),
            header: header.iter().map(strip_custom).collect(),
            rows: rows.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::TableRow { cells, source } => Node::TableRow {
            cells: cells.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::TableCell {
            children,
            header,
            alignment,
            source,
        } => Node::TableCell {
            children: children.iter().map(strip_custom).collect(),
            header: *header,
            alignment: *alignment,
            source: *source,
        },
        Node::Text { text, source } => Node::Text {
            text: text.clone(),
            source: *source,
        },
        Node::Emphasis { children, source } => Node::Emphasis {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Strong { children, source } => Node::Strong {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Strikethrough { children, source } => Node::Strikethrough {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Subscript { children, source } => Node::Subscript {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Superscript { children, source } => Node::Superscript {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Highlight { children, source } => Node::Highlight {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Link {
            destination,
            title,
            children,
            source,
        } => Node::Link {
            destination: destination.clone(),
            title: title.clone(),
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Image {
            destination,
            title,
            alt,
            source,
        } => Node::Image {
            destination: destination.clone(),
            title: title.clone(),
            alt: alt.clone(),
            source: *source,
        },
        Node::CodeSpan { code, source } => Node::CodeSpan {
            code: code.clone(),
            source: *source,
        },
        Node::CodeBlock {
            info,
            literal,
            source,
        } => Node::CodeBlock {
            info: info.clone(),
            literal: literal.clone(),
            source: *source,
        },
        Node::MathInline { math, source } => Node::MathInline {
            math: math.clone(),
            source: *source,
        },
        Node::MathBlock { math, source } => Node::MathBlock {
            math: math.clone(),
            source: *source,
        },
        Node::Html { html, source } => Node::Html {
            html: html.clone(),
            source: *source,
        },
        Node::SoftBreak { source } => Node::SoftBreak { source: *source },
        Node::HardBreak { source } => Node::HardBreak { source: *source },
        Node::ThematicBreak { source } => Node::ThematicBreak { source: *source },
        Node::FootnoteRef {
            label,
            number,
            ref_num,
            source,
        } => Node::FootnoteRef {
            label: label.clone(),
            number: *number,
            ref_num: *ref_num,
            source: *source,
        },
        Node::FootnoteDefinition {
            label,
            number,
            backref_count,
            children,
            source,
        } => Node::FootnoteDefinition {
            label: label.clone(),
            number: *number,
            backref_count: *backref_count,
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::FootnoteSection { children, source } => Node::FootnoteSection {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Alert {
            kind,
            title,
            children,
            source,
        } => Node::Alert {
            kind: *kind,
            title: title.clone(),
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::DefinitionList { children, source } => Node::DefinitionList {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::DefinitionTerm { children, source } => Node::DefinitionTerm {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::DefinitionDescription { children, source } => Node::DefinitionDescription {
            children: children.iter().map(strip_custom).collect(),
            source: *source,
        },
        Node::Custom {
            children, source, ..
        } => {
            // Drop the custom payload; concatenate children as a Document fragment.
            Node::Document {
                children: children.iter().map(strip_custom).collect(),
                source: *source,
            }
        }
    }
}

fn rewrite_inline_code(node: &mut Node<TraceyExt>, handler: &dyn InlineCodeHandler) {
    match node {
        Node::CodeSpan { code, source } => {
            if let Some(html) = handler.render(code) {
                let span = *source;
                *node = Node::Html { html, source: span };
            }
        }
        Node::Document { children, .. }
        | Node::Paragraph { children, .. }
        | Node::Heading { children, .. }
        | Node::BlockQuote { children, .. }
        | Node::List { children, .. }
        | Node::Item { children, .. }
        | Node::TaskItem { children, .. }
        | Node::Emphasis { children, .. }
        | Node::Strong { children, .. }
        | Node::Strikethrough { children, .. }
        | Node::Subscript { children, .. }
        | Node::Superscript { children, .. }
        | Node::Highlight { children, .. }
        | Node::Link { children, .. }
        | Node::FootnoteDefinition { children, .. }
        | Node::FootnoteSection { children, .. }
        | Node::Alert { children, .. }
        | Node::DefinitionList { children, .. }
        | Node::DefinitionTerm { children, .. }
        | Node::DefinitionDescription { children, .. }
        | Node::Custom { children, .. } => {
            for child in children.iter_mut() {
                rewrite_inline_code(child, handler);
            }
        }
        Node::Table { header, rows, .. } => {
            for row in header.iter_mut() {
                rewrite_inline_code(row, handler);
            }
            for row in rows.iter_mut() {
                rewrite_inline_code(row, handler);
            }
        }
        Node::TableRow { cells, .. } => {
            for cell in cells.iter_mut() {
                rewrite_inline_code(cell, handler);
            }
        }
        Node::TableCell { children, .. } => {
            for child in children.iter_mut() {
                rewrite_inline_code(child, handler);
            }
        }
        _ => {}
    }
}

fn replace_reqs_with_html<'a>(
    node: &mut Node<TraceyExt>,
    inner_adapter: &HtmlAdapter<()>,
    handler: &dyn ReqHandler,
    reqs: &mut std::slice::Iter<'a, ReqDefinition>,
) -> Result<(), RenderError> {
    if let Node::Custom {
        payload: TraceyExt::Req(_),
        children,
        source,
    } = node
    {
        let req = reqs.next().expect("reqs vec drift").clone();
        let mut out = String::new();
        out.push_str(&handler.start(&req)?);
        // Render children to HTML using inner adapter (custom-free).
        for child in children.iter() {
            let cloned: Node<()> = strip_custom(child);
            let item = StreamItem::Upsert(crate::node::Emission {
                id: 0,
                provisional: false,
                node: cloned,
            });
            if let Ok(StreamItem::Upsert(e)) = inner_adapter.render(item) {
                out.push_str(&e.node);
            }
        }
        out.push_str(&handler.end(&req)?);
        let span = *source;
        *node = Node::Html {
            html: out,
            source: span,
        };
        return Ok(());
    }

    match node {
        Node::Document { children, .. }
        | Node::BlockQuote { children, .. }
        | Node::List { children, .. }
        | Node::Item { children, .. }
        | Node::TaskItem { children, .. }
        | Node::FootnoteDefinition { children, .. }
        | Node::FootnoteSection { children, .. }
        | Node::Alert { children, .. }
        | Node::DefinitionList { children, .. }
        | Node::DefinitionTerm { children, .. }
        | Node::DefinitionDescription { children, .. } => {
            for child in children.iter_mut() {
                replace_reqs_with_html(child, inner_adapter, handler, reqs)?;
            }
        }
        Node::Table { header, rows, .. } => {
            for row in header.iter_mut() {
                replace_reqs_with_html(row, inner_adapter, handler, reqs)?;
            }
            for row in rows.iter_mut() {
                replace_reqs_with_html(row, inner_adapter, handler, reqs)?;
            }
        }
        _ => {}
    }
    Ok(())
}
