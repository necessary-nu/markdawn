//! Footnote machinery: recognising and collecting `[^label]:`
//! definitions out of the raw line buffer, then numbering the
//! `FootnoteRef` nodes a parsed tree carries.

use super::{
    RawFootnoteDef, RawFootnoteDefs, classify_html_block_start, is_thematic_break, leading_indent,
    parse_fence_opener, parse_list_marker, strip_block_quote_marker, strip_indent_columns,
    trim_up_to_three_spaces,
};
use crate::node::Node;
use std::collections::HashMap;

// Detects the start of a footnote definition: `[^label]:` (with up to 3 leading spaces).
// Returns (label, byte_offset_of_content_after_colon_on_first_line).
fn parse_footnote_def_opener(line: &str) -> Option<(String, usize)> {
    let trimmed = trim_up_to_three_spaces(line);
    let indent_bytes = line.len() - trimmed.len();
    let after_bracket = trimmed.strip_prefix("[^")?;
    let label_end = find_footnote_label_end(after_bracket)?;
    let label = &after_bracket[..label_end];
    if label.is_empty() || label.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let after_label = &after_bracket[label_end + 1..];
    if !after_label.starts_with(':') {
        return None;
    }
    let content_offset = indent_bytes + 2 + label_end + 1 + 1;
    Some((label.to_string(), content_offset))
}

pub(super) fn find_footnote_label_end(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == ']' {
            return Some(i);
        }
        if ch == '[' || ch == '\n' {
            return None;
        }
    }
    None
}

// Collect all raw lines belonging to a footnote definition starting at `start`.
// Strips the opener prefix and 4 columns of indentation from continuation lines.
// Returns (next_index_after_def, label, body_lines).
pub(super) fn collect_footnote_def_lines(
    lines: &[String],
    start: usize,
) -> Option<(usize, String, Vec<String>)> {
    let (label, content_offset) = parse_footnote_def_opener(&lines[start])?;
    let first_content = lines[start][content_offset..].trim_start().to_string();
    let mut body: Vec<String> = vec![first_content];

    let mut index = start + 1;
    let mut pending_blanks: usize = 0;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() {
            pending_blanks += 1;
            index += 1;
            continue;
        }
        let indent = leading_indent(line);
        if indent.columns >= 4 {
            for _ in 0..pending_blanks {
                body.push(String::new());
            }
            pending_blanks = 0;
            let stripped = strip_indent_columns(line, 4).unwrap_or_else(|| line.clone());
            body.push(stripped);
            index += 1;
            continue;
        }
        break;
    }

    // Rewind past trailing blanks inside the footnote — those belong to the surrounding document.
    let rewind_to = index - pending_blanks;
    Some((rewind_to, label, body))
}

pub(super) fn collect_raw_footnote_definitions(lines: &[String]) -> RawFootnoteDefs {
    let mut defs: RawFootnoteDefs = HashMap::new();
    let mut index = 0;
    let mut in_paragraph = false;
    while index < lines.len() {
        if lines[index].trim().is_empty() {
            in_paragraph = false;
            index += 1;
            continue;
        }
        if !in_paragraph && let Some((next, label, body)) = collect_footnote_def_lines(lines, index)
        {
            defs.entry(label).or_insert(RawFootnoteDef {
                lines: body,
                line_range: index..next,
            });
            index = next;
            continue;
        }
        // Conservatively treat other lines as potentially paragraph text so that
        // a `[^label]:` appearing mid-paragraph doesn't steal content.
        in_paragraph = !is_thematic_break(&lines[index])
            && classify_html_block_start(&lines[index]).is_none()
            && parse_fence_opener(&lines[index]).is_none()
            && !lines[index].trim_start().starts_with('#')
            && strip_block_quote_marker(&lines[index]).is_none()
            && parse_list_marker(&lines[index]).is_none();
        index += 1;
    }
    defs
}

pub(super) fn collect_footnote_refs<Ext>(
    node: &Node<Ext>,
    ordered_labels: &mut Vec<String>,
    occurrences: &mut HashMap<String, u32>,
) {
    match node {
        Node::FootnoteRef { label, .. } => {
            let count = occurrences.entry(label.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                ordered_labels.push(label.clone());
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
        | Node::Link { children, .. }
        | Node::Custom { children, .. } => {
            for child in children {
                collect_footnote_refs(child, ordered_labels, occurrences);
            }
        }
        Node::Table { header, rows, .. } => {
            for n in header {
                collect_footnote_refs(n, ordered_labels, occurrences);
            }
            for n in rows {
                collect_footnote_refs(n, ordered_labels, occurrences);
            }
        }
        Node::TableRow { cells, .. } => {
            for n in cells {
                collect_footnote_refs(n, ordered_labels, occurrences);
            }
        }
        Node::TableCell { children, .. } => {
            for child in children {
                collect_footnote_refs(child, ordered_labels, occurrences);
            }
        }
        // FootnoteDefinition / FootnoteSection only appear at top level after
        // post-processing; if encountered here we still recurse so nested refs
        // inside a definition's content get counted.
        Node::FootnoteDefinition { children, .. } | Node::FootnoteSection { children, .. } => {
            for child in children {
                collect_footnote_refs(child, ordered_labels, occurrences);
            }
        }
        _ => {}
    }
}

pub(super) fn assign_footnote_ref_numbers<Ext>(
    node: &mut Node<Ext>,
    label_numbers: &HashMap<String, u32>,
    counters: &mut HashMap<String, u32>,
) {
    match node {
        Node::FootnoteRef {
            label,
            number,
            ref_num,
            ..
        } => {
            if let Some(n) = label_numbers.get(label) {
                *number = *n;
            }
            let counter = counters.entry(label.clone()).or_insert(0);
            *counter += 1;
            *ref_num = *counter;
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
        | Node::Link { children, .. }
        | Node::Custom { children, .. }
        | Node::FootnoteDefinition { children, .. }
        | Node::FootnoteSection { children, .. } => {
            for child in children {
                assign_footnote_ref_numbers(child, label_numbers, counters);
            }
        }
        Node::Table { header, rows, .. } => {
            for n in header {
                assign_footnote_ref_numbers(n, label_numbers, counters);
            }
            for n in rows {
                assign_footnote_ref_numbers(n, label_numbers, counters);
            }
        }
        Node::TableRow { cells, .. } => {
            for n in cells {
                assign_footnote_ref_numbers(n, label_numbers, counters);
            }
        }
        Node::TableCell { children, .. } => {
            for child in children {
                assign_footnote_ref_numbers(child, label_numbers, counters);
            }
        }
        _ => {}
    }
}
