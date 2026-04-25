//! Custom-node payload type for tracey reqs and the post-parse extractor that
//! turns paragraph/blockquote markers into [`Node::Custom`] nodes.

use crate::node::{Node, SourceSpan};

use super::heading::slugify;
use super::req::{RuleId, parse_req_leading_marker, parse_rule_id};

/// Custom-node payload used inside `Node::Custom { payload: TraceyExt::Req(_), .. }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceyExt {
    Req(ReqPayload),
}

/// Per-req metadata carried on the Custom node.
///
/// Source spans are *byte ranges in the original markdown source*. They are
/// computed at extraction time and stay valid through subsequent AST walks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReqPayload {
    pub id: RuleId,
    pub anchor_id: String,
    /// Span of the marker (e.g. `r[my.req]`).
    pub marker_offset: usize,
    pub marker_length: usize,
    /// 1-indexed line number where the marker appears.
    pub line: usize,
    /// Original `prefix` consumed (e.g. `"r"`).
    pub prefix: String,
}

/// Walk the AST and convert paragraphs / blockquotes whose first text starts
/// with a `prefix[id]` marker into `Node::Custom { TraceyExt::Req(_), … }`.
///
/// `source` is the full markdown body that markdawn parsed. `base_offset` is
/// the byte offset of `source` within an outer document (used to shift node
/// `SourceSpan`s when frontmatter was stripped before parsing).
pub fn extract_reqs<Ext: From<TraceyExt> + Clone>(
    nodes: &mut [Node<Ext>],
    source: &str,
    base_offset: usize,
) {
    for node in nodes.iter_mut() {
        try_convert_to_req(node, source, base_offset);
        recurse_children(node, source, base_offset);
    }
}

fn recurse_children<Ext: From<TraceyExt> + Clone>(
    node: &mut Node<Ext>,
    source: &str,
    base_offset: usize,
) {
    match node {
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
            extract_reqs(children, source, base_offset);
        }
        Node::Table { header, rows, .. } => {
            extract_reqs(header, source, base_offset);
            extract_reqs(rows, source, base_offset);
        }
        Node::TableRow { cells, .. } => {
            extract_reqs(cells, source, base_offset);
        }
        Node::TableCell { children, .. } => {
            extract_reqs(children, source, base_offset);
        }
        _ => {}
    }
}

fn try_convert_to_req<Ext: From<TraceyExt> + Clone>(
    node: &mut Node<Ext>,
    source: &str,
    base_offset: usize,
) {
    match node {
        // Paragraph form: `prefix[id] body…`
        Node::Paragraph { children, source: span } => {
            let abs_start = span.start + base_offset;
            let abs_end = span.end + base_offset;
            let Some((payload, marker_byte_len, body_start_in_block)) =
                detect_marker_in_paragraph_block(source, abs_start, abs_end)
            else {
                return;
            };

            let body_text = &source[abs_start + body_start_in_block..abs_end];
            let trimmed_body = body_text.trim_start_matches(|c: char| c == ' ' || c == '\t');
            // Replace the leading Text child(ren) representing the marker with a
            // version that has the marker removed. Simpler: rebuild children
            // from the body text via inline reparse — but we don't have the
            // inline parser here. Instead, walk the existing inline children
            // and strip the marker from the first Text node.
            strip_leading_marker_from_inlines(children, marker_byte_len);
            let _ = trimmed_body;

            let custom_payload = Ext::from(TraceyExt::Req(payload));
            let new_children = std::mem::take(children);
            let new_span = *span;
            *node = Node::Custom {
                payload: custom_payload,
                children: new_children,
                source: new_span,
            };
        }
        // Blockquote form: `> prefix[id] body…`
        Node::BlockQuote { children, source: span } => {
            // The first inner child of a blockquote should be a Paragraph
            // whose first text starts with the marker.
            let abs_start = span.start + base_offset;
            // The marker is on the first non-blank line; find it by scanning
            // source for `[a-z0-9]+\[…\]` after any `> ` prefixes.
            let block_text = &source[abs_start..span.end + base_offset];
            let Some((marker_offset_in_block, marker_byte_len, payload)) =
                detect_marker_in_blockquote_block(block_text, abs_start)
            else {
                return;
            };

            // The marker is inside the first inner Paragraph. Strip it from
            // that paragraph's first Text node.
            if let Some(first_para) = children.first_mut()
                && let Node::Paragraph { children: para_children, .. } = first_para
            {
                strip_leading_marker_from_inlines(para_children, marker_byte_len);
            }
            let _ = marker_offset_in_block;

            let custom_payload = Ext::from(TraceyExt::Req(payload));
            let new_children = std::mem::take(children);
            let new_span = *span;
            *node = Node::Custom {
                payload: custom_payload,
                children: new_children,
                source: new_span,
            };
        }
        _ => {}
    }
}

/// Look for a paragraph-form marker within the byte range `[abs_start, abs_end)`
/// of `source`. On success returns `(payload, marker_byte_len, body_start)`
/// where `body_start` is the offset within the paragraph block at which the
/// post-marker body starts.
fn detect_marker_in_paragraph_block(
    source: &str,
    abs_start: usize,
    abs_end: usize,
) -> Option<(ReqPayload, usize, usize)> {
    let block = source.get(abs_start..abs_end)?;
    let (prefix, marker_content, marker_end_idx) = parse_req_leading_marker(block)?;
    let marker_byte_len = marker_end_idx + 1; // includes closing ']'

    let id = parse_rule_id(marker_content)?;
    let anchor_id = format!("r--{}", slugify(&id.to_string()));

    let line = 1 + source[..abs_start].bytes().filter(|&b| b == b'\n').count();

    Some((
        ReqPayload {
            id,
            anchor_id,
            marker_offset: abs_start,
            marker_length: marker_byte_len,
            line,
            prefix: prefix.to_string(),
        },
        marker_byte_len,
        marker_byte_len,
    ))
}

/// Look for a blockquote-form marker. `block_text` is the entire blockquote
/// source (including `> ` prefixes). Returns the marker offset *in the
/// original source* via `abs_start` + relative.
fn detect_marker_in_blockquote_block(
    block_text: &str,
    abs_start: usize,
) -> Option<(usize, usize, ReqPayload)> {
    // Find the first non-blockquote-prefix character on the first line.
    let first_line_end = block_text.find('\n').unwrap_or(block_text.len());
    let first_line = &block_text[..first_line_end];

    // Strip leading whitespace, then `>`, then any single space.
    let mut idx = 0;
    while idx < first_line.len() && matches!(first_line.as_bytes()[idx], b' ' | b'\t') {
        idx += 1;
    }
    if first_line.as_bytes().get(idx) != Some(&b'>') {
        return None;
    }
    idx += 1;
    if first_line.as_bytes().get(idx) == Some(&b' ') {
        idx += 1;
    }

    let marker_text = &first_line[idx..];
    let (prefix, marker_content, marker_end_idx) = parse_req_leading_marker(marker_text)?;
    let marker_byte_len = marker_end_idx + 1;
    let id = parse_rule_id(marker_content)?;
    let anchor_id = format!("r--{}", slugify(&id.to_string()));

    let marker_offset = abs_start + idx;
    // Compute line number: count newlines before abs_start in the document.
    // Caller will need to do this too; we just return marker_offset.
    Some((
        marker_offset,
        marker_byte_len,
        ReqPayload {
            id,
            anchor_id,
            marker_offset,
            marker_length: marker_byte_len,
            line: 0, // filled in below
            prefix: prefix.to_string(),
        },
    ))
}

/// Strip the leading `marker_byte_len` bytes from the inline children of a
/// paragraph. Walks `Text` nodes in order, shrinking them until we've removed
/// `marker_byte_len` bytes.
fn strip_leading_marker_from_inlines<Ext>(children: &mut Vec<Node<Ext>>, mut to_strip: usize) {
    if to_strip == 0 {
        return;
    }
    let mut i = 0;
    while i < children.len() && to_strip > 0 {
        match &mut children[i] {
            Node::Text { text, source } => {
                let take = to_strip.min(text.len());
                let new = text[take..].to_string();
                let trimmed = new.trim_start_matches(|c: char| c == ' ' || c == '\t');
                let drop_extra = new.len() - trimmed.len();
                let new = trimmed.to_string();
                *source = SourceSpan::new(source.start + take + drop_extra, source.end);
                if new.is_empty() {
                    children.remove(i);
                    to_strip = to_strip.saturating_sub(take + drop_extra);
                    continue;
                }
                *text = new;
                to_strip = to_strip.saturating_sub(take + drop_extra);
                i += 1;
            }
            _ => break,
        }
    }
}
