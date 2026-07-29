//! Block-level half of the serializer: one writer per block kind,
//! dispatched from [`write_block`]. Inline emission, escaping and the
//! public entry points live in the parent `serializer` module.

use super::{InlineCtx, WriteCtx, ensure_blank_line, write_inline, write_inlines};
use crate::node::{AlertKind, Node, TableAlignment};

// [spec:mw:sem:editing.serialize.block-forms+1]
pub(super) fn write_block<Ext>(node: &Node<Ext>, out: &mut String, ctx: &mut WriteCtx) {
    match node {
        Node::Document { children, .. } => write_children_spaced(children, out, ctx),
        Node::Paragraph { children, .. } => {
            write_inlines(children, out, &mut InlineCtx::flow());
            out.push('\n');
        }
        Node::Heading {
            level, children, ..
        } => write_heading(*level, children, out),
        Node::BlockQuote { children, .. } => write_block_quote(children, out, ctx),
        Node::Alert {
            kind,
            title,
            children,
            ..
        } => write_alert(kind, title.as_deref(), children, out, ctx),
        Node::List {
            ordered,
            start,
            children,
            ..
        } => write_list(*ordered, *start, children, out, ctx),
        // Fallback: shouldn't normally be top-level.
        Node::Item { .. } => write_list_item(node, out, ctx, "- "),
        Node::TaskItem {
            checked, children, ..
        } => {
            let marker = if *checked { "- [x] " } else { "- [ ] " };
            write_marked_block(children, out, ctx, marker, "  ");
        }
        Node::CodeBlock { info, literal, .. } => write_code_block(info.as_deref(), literal, out),
        Node::MathBlock { math, .. } => write_math_block(math, out),
        Node::ThematicBreak { .. } => out.push_str("---\n"),
        Node::Html { html, .. } => write_html_block(html, out),
        Node::Table {
            alignments,
            header,
            rows,
            ..
        } => write_table(alignments, header, rows, out),
        Node::FootnoteSection { children, .. } | Node::FootnoteDefinition { children, .. } => {
            write_children_spaced(children, out, ctx)
        }
        Node::DefinitionList { children, .. } => {
            for child in children {
                write_block(child, out, ctx);
            }
        }
        Node::DefinitionTerm { children, .. } => {
            write_inlines(children, out, &mut InlineCtx::flow());
            out.push('\n');
        }
        Node::DefinitionDescription { children, .. } => {
            write_definition_description(children, out, ctx)
        }
        Node::TableRow { cells, .. } => write_table_row(cells, out),
        Node::TableCell { children, .. } => {
            write_inlines(children, out, &mut InlineCtx::flow());
            out.push('\n');
        }
        Node::Custom { children, .. } => {
            for child in children {
                write_block(child, out, ctx);
            }
        }
        // Inline nodes encountered at block position start their own line.
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
        | Node::MathInline { .. }
        | Node::SoftBreak { .. }
        | Node::HardBreak { .. }
        | Node::FootnoteRef { .. } => write_inline(node, out, &mut InlineCtx::flow()),
    }
}

/// Blocks emitted one after another with the blank-line separator top-level
/// blocks get.
fn write_children_spaced<Ext>(children: &[Node<Ext>], out: &mut String, ctx: &mut WriteCtx) {
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            ensure_blank_line(out);
        }
        write_block(child, out, ctx);
    }
}

/// Children rendered to their own buffer against a cloned context, so the
/// caller can prefix or indent the result line by line.
fn nested_blocks<Ext>(children: &[Node<Ext>], ctx: &WriteCtx) -> String {
    let mut inner = String::new();
    let mut inner_ctx = ctx.clone();
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            ensure_blank_line(&mut inner);
        }
        write_block(child, &mut inner, &mut inner_ctx);
    }
    inner
}

fn write_heading<Ext>(level: u8, children: &[Node<Ext>], out: &mut String) {
    for _ in 0..level.min(6) {
        out.push('#');
    }
    out.push(' ');
    write_inlines(children, out, &mut InlineCtx::mid_line());
    out.push('\n');
}

fn write_block_quote<Ext>(children: &[Node<Ext>], out: &mut String, ctx: &mut WriteCtx) {
    ctx.bq_depth += 1;
    let inner = nested_blocks(children, ctx);
    write_quoted_lines(&inner, out);
    ctx.bq_depth -= 1;
}

fn write_alert<Ext>(
    kind: &AlertKind,
    title: Option<&str>,
    children: &[Node<Ext>],
    out: &mut String,
    ctx: &mut WriteCtx,
) {
    ctx.bq_depth += 1;
    let tag = match kind {
        AlertKind::Note => "[!NOTE]",
        AlertKind::Tip => "[!TIP]",
        AlertKind::Important => "[!IMPORTANT]",
        AlertKind::Warning => "[!WARNING]",
        AlertKind::Caution => "[!CAUTION]",
    };
    out.push_str("> ");
    out.push_str(tag);
    if let Some(custom) = title {
        out.push(' ');
        out.push_str(custom);
    }
    out.push('\n');
    let inner = nested_blocks(children, ctx);
    write_quoted_lines(&inner, out);
    ctx.bq_depth -= 1;
}

fn write_quoted_lines(inner: &str, out: &mut String) {
    for line in inner.lines() {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
}

fn write_list<Ext>(
    ordered: bool,
    start: Option<u64>,
    children: &[Node<Ext>],
    out: &mut String,
    ctx: &mut WriteCtx,
) {
    let start_num = start.unwrap_or(1);
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let marker = if ordered {
            format!("{}. ", start_num + i as u64)
        } else {
            "- ".to_string()
        };
        write_list_item(child, out, ctx, &marker);
    }
}

fn write_list_item<Ext>(node: &Node<Ext>, out: &mut String, ctx: &mut WriteCtx, marker: &str) {
    let children = match node {
        Node::Item { children, .. } | Node::TaskItem { children, .. } => children,
        _ => return,
    };
    let marker = match node {
        Node::TaskItem { checked, .. } => {
            if *checked {
                "- [x] "
            } else {
                "- [ ] "
            }
        }
        _ => marker,
    };
    write_marked_block(children, out, ctx, marker, &" ".repeat(marker.len()));
}

/// A nested block sequence introduced by `marker`, with every continuation
/// line carrying `indent`.
fn write_marked_block<Ext>(
    children: &[Node<Ext>],
    out: &mut String,
    ctx: &mut WriteCtx,
    marker: &str,
    indent: &str,
) {
    ctx.list_depth += 1;
    let inner = nested_blocks(children, ctx);
    ctx.list_depth -= 1;
    write_marked_lines(&inner, out, marker, indent);
}

fn write_marked_lines(inner: &str, out: &mut String, marker: &str, indent: &str) {
    let mut lines = inner.lines();
    if let Some(first) = lines.next() {
        out.push_str(marker);
        out.push_str(first);
        out.push('\n');
    }
    for line in lines {
        out.push_str(indent);
        out.push_str(line);
        out.push('\n');
    }
}

fn write_definition_description<Ext>(children: &[Node<Ext>], out: &mut String, ctx: &mut WriteCtx) {
    let mut inner = String::new();
    let mut inner_ctx = ctx.clone();
    for child in children {
        write_block(child, &mut inner, &mut inner_ctx);
    }
    write_marked_lines(&inner, out, ": ", "  ");
}

fn write_math_block(math: &str, out: &mut String) {
    out.push_str("$$\n");
    out.push_str(math);
    if !math.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("$$\n");
}

fn write_html_block(html: &str, out: &mut String) {
    out.push_str(html);
    if !html.ends_with('\n') {
        out.push('\n');
    }
}

// [spec:mw:req:editing.serialize.escaping+1]
fn write_code_block(info: Option<&str>, literal: &str, out: &mut String) {
    let marker = if info.is_some_and(|info| info.contains('`')) {
        '~'
    } else {
        '`'
    };
    let fence = fence_width(literal, marker);
    for _ in 0..fence {
        out.push(marker);
    }
    if let Some(info) = info {
        out.push_str(info);
    }
    out.push('\n');
    out.push_str(literal);
    if !literal.ends_with('\n') {
        out.push('\n');
    }
    for _ in 0..fence {
        out.push(marker);
    }
    out.push('\n');
}

/// Widen the fence past any line of `literal` that would otherwise read as a
/// closing fence.
fn fence_width(literal: &str, marker: char) -> usize {
    let mut fence = 3usize;
    for line in literal.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent > 3 {
            continue;
        }
        let body = &line[indent..];
        let run = body.chars().take_while(|ch| *ch == marker).count();
        if run > 0 && body[run..].trim().is_empty() {
            fence = fence.max(run + 1);
        }
    }
    fence
}

fn write_table<Ext>(
    alignments: &[Option<TableAlignment>],
    header: &[Node<Ext>],
    rows: &[Node<Ext>],
    out: &mut String,
) {
    if let Some(Node::TableRow { cells, .. }) = header.first() {
        write_table_row(cells, out);
    }
    out.push('|');
    for align in alignments {
        match align {
            Some(TableAlignment::Left) => out.push_str(":---"),
            Some(TableAlignment::Center) => out.push_str(":---:"),
            Some(TableAlignment::Right) => out.push_str("---:"),
            None => out.push_str("---"),
        }
        out.push('|');
    }
    out.push('\n');
    for row in rows {
        if let Node::TableRow { cells, .. } = row {
            write_table_row(cells, out);
        }
    }
}

// [spec:mw:req:editing.serialize.escaping+1]
fn write_table_row<Ext>(cells: &[Node<Ext>], out: &mut String) {
    out.push('|');
    for cell in cells {
        out.push(' ');
        let mut body = String::new();
        if let Node::TableCell { children, .. } = cell {
            write_inlines(children, &mut body, &mut InlineCtx::mid_line());
        }
        write_cell_body(&body, out);
        out.push_str(" |");
    }
    out.push('\n');
}

/// A pipe anywhere in a rendered cell — including inside a code span or a
/// link target — splits the row, so every one is escaped once the cell's
/// inlines have been rendered. Row splitting happens before inline parsing,
/// so the escape is undone before the cell body is read back.
// [spec:mw:req:editing.serialize.escaping+1]
fn write_cell_body(rendered: &str, out: &mut String) {
    for ch in rendered.chars() {
        match ch {
            '|' => out.push_str("\\|"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
}
