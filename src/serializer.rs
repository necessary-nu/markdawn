//! Render `Node` trees back to canonical markdown source.
//!
//! Output is canonical, not byte-identical to the input. Round-tripping
//! source → parse → serialize → parse produces the same AST, but the
//! serialized text may differ (e.g., emphasis always uses `*`, headings
//! always use ATX `#` form, list bullets always use `-`).
//!
//! ```ignore
//! let source = "# Hello\n\nWorld";
//! let blocks = parser.parse_full(source).unwrap();
//! let out: String = blocks.iter().map(|b| serialize(&b.node)).collect();
//! ```

use crate::node::{AlertKind, Node, TableAlignment};

/// Serialize a single node (block or inline) to canonical markdown.
pub fn serialize<Ext>(node: &Node<Ext>) -> String {
    let mut out = String::new();
    write_block(node, &mut out, &mut WriteCtx::default());
    out
}

/// Serialize a sequence of top-level blocks to canonical markdown, joined
/// with blank-line separators.
pub fn serialize_blocks<Ext>(blocks: &[Node<Ext>]) -> String {
    let mut out = String::new();
    let mut ctx = WriteCtx::default();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            ensure_blank_line(&mut out);
        }
        write_block(block, &mut out, &mut ctx);
    }
    out
}

#[derive(Default, Clone)]
struct WriteCtx {
    /// Current list-item nesting level (0 = top-level).
    list_depth: u32,
    /// Block-quote nesting depth (0 = none).
    bq_depth: u32,
}

fn write_block<Ext>(node: &Node<Ext>, out: &mut String, ctx: &mut WriteCtx) {
    match node {
        Node::Document { children, .. } => {
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    ensure_blank_line(out);
                }
                write_block(child, out, ctx);
            }
        }
        Node::Paragraph { children, .. } => {
            write_inlines(children, out);
            out.push('\n');
        }
        Node::Heading {
            level, children, ..
        } => {
            for _ in 0..(*level).min(6) {
                out.push('#');
            }
            out.push(' ');
            write_inlines(children, out);
            out.push('\n');
        }
        Node::BlockQuote { children, .. } => {
            ctx.bq_depth += 1;
            let mut inner = String::new();
            let mut inner_ctx = ctx.clone();
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    ensure_blank_line(&mut inner);
                }
                write_block(child, &mut inner, &mut inner_ctx);
            }
            for line in inner.lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            ctx.bq_depth -= 1;
        }
        Node::Alert { kind, children, .. } => {
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
            out.push('\n');
            let mut inner = String::new();
            let mut inner_ctx = ctx.clone();
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    ensure_blank_line(&mut inner);
                }
                write_block(child, &mut inner, &mut inner_ctx);
            }
            for line in inner.lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            ctx.bq_depth -= 1;
        }
        Node::List {
            ordered,
            start,
            children,
            ..
        } => {
            let start_num = start.unwrap_or(1);
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                let marker = if *ordered {
                    format!("{}. ", start_num + i as u64)
                } else {
                    "- ".to_string()
                };
                write_list_item(child, out, ctx, &marker);
            }
        }
        Node::Item { children, .. } => {
            write_list_item(node, out, ctx, "- ");
            // Fallback: shouldn't normally be top-level.
            let _ = children;
        }
        Node::TaskItem {
            checked, children, ..
        } => {
            let marker = if *checked { "- [x] " } else { "- [ ] " };
            ctx.list_depth += 1;
            let mut inner = String::new();
            let mut inner_ctx = ctx.clone();
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    ensure_blank_line(&mut inner);
                }
                write_block(child, &mut inner, &mut inner_ctx);
            }
            ctx.list_depth -= 1;
            let mut lines = inner.lines();
            if let Some(first) = lines.next() {
                out.push_str(marker);
                out.push_str(first);
                out.push('\n');
            }
            for line in lines {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
        Node::CodeBlock { info, literal, .. } => {
            out.push_str("```");
            if let Some(info) = info {
                out.push_str(info);
            }
            out.push('\n');
            out.push_str(literal);
            if !literal.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n");
        }
        Node::MathBlock { math, .. } => {
            out.push_str("$$\n");
            out.push_str(math);
            if !math.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("$$\n");
        }
        Node::ThematicBreak { .. } => {
            out.push_str("---\n");
        }
        Node::Html { html, .. } => {
            out.push_str(html);
            if !html.ends_with('\n') {
                out.push('\n');
            }
        }
        Node::Table {
            alignments,
            header,
            rows,
            ..
        } => write_table(alignments, header, rows, out),
        Node::FootnoteSection { children, .. } | Node::FootnoteDefinition { children, .. } => {
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    ensure_blank_line(out);
                }
                write_block(child, out, ctx);
            }
        }
        Node::DefinitionList { children, .. } => {
            for child in children {
                write_block(child, out, ctx);
            }
        }
        Node::DefinitionTerm { children, .. } => {
            write_inlines(children, out);
            out.push('\n');
        }
        Node::DefinitionDescription { children, .. } => {
            let mut inner = String::new();
            let mut inner_ctx = ctx.clone();
            for child in children {
                write_block(child, &mut inner, &mut inner_ctx);
            }
            let mut lines = inner.lines();
            if let Some(first) = lines.next() {
                out.push_str(": ");
                out.push_str(first);
                out.push('\n');
            }
            for line in lines {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
        // Inline nodes encountered at block position fall through to inline.
        _ => write_inlines(std::slice::from_ref(node), out),
    }
}

fn write_list_item<Ext>(node: &Node<Ext>, out: &mut String, ctx: &mut WriteCtx, marker: &str) {
    let children = match node {
        Node::Item { children, .. } | Node::TaskItem { children, .. } => children,
        _ => return,
    };
    if let Node::TaskItem { checked, .. } = node {
        let m = if *checked { "- [x] " } else { "- [ ] " };
        write_block_list_inner(children, out, ctx, m);
    } else {
        write_block_list_inner(children, out, ctx, marker);
    }
}

fn write_block_list_inner<Ext>(
    children: &[Node<Ext>],
    out: &mut String,
    ctx: &mut WriteCtx,
    marker: &str,
) {
    ctx.list_depth += 1;
    let mut inner = String::new();
    let mut inner_ctx = ctx.clone();
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            ensure_blank_line(&mut inner);
        }
        write_block(child, &mut inner, &mut inner_ctx);
    }
    ctx.list_depth -= 1;

    let indent = " ".repeat(marker.len());
    let mut lines = inner.lines();
    if let Some(first) = lines.next() {
        out.push_str(marker);
        out.push_str(first);
        out.push('\n');
    }
    for line in lines {
        out.push_str(&indent);
        out.push_str(line);
        out.push('\n');
    }
}

fn write_table<Ext>(
    alignments: &[Option<TableAlignment>],
    header: &[Node<Ext>],
    rows: &[Node<Ext>],
    out: &mut String,
) {
    let cols = alignments.len();
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
    let _ = cols;
}

fn write_table_row<Ext>(cells: &[Node<Ext>], out: &mut String) {
    out.push('|');
    for cell in cells {
        out.push(' ');
        if let Node::TableCell { children, .. } = cell {
            write_inlines(children, out);
        }
        out.push_str(" |");
    }
    out.push('\n');
}

fn write_inlines<Ext>(nodes: &[Node<Ext>], out: &mut String) {
    for node in nodes {
        write_inline(node, out);
    }
}

fn write_inline<Ext>(node: &Node<Ext>, out: &mut String) {
    match node {
        Node::Text { text, .. } => out.push_str(text),
        Node::Emphasis { children, .. } => {
            out.push('*');
            write_inlines(children, out);
            out.push('*');
        }
        Node::Strong { children, .. } => {
            out.push_str("**");
            write_inlines(children, out);
            out.push_str("**");
        }
        Node::Strikethrough { children, .. } => {
            out.push_str("~~");
            write_inlines(children, out);
            out.push_str("~~");
        }
        Node::Subscript { children, .. } => {
            out.push_str("<sub>");
            write_inlines(children, out);
            out.push_str("</sub>");
        }
        Node::Superscript { children, .. } => {
            out.push_str("<sup>");
            write_inlines(children, out);
            out.push_str("</sup>");
        }
        Node::Highlight { children, .. } => {
            out.push_str("==");
            write_inlines(children, out);
            out.push_str("==");
        }
        Node::Link {
            destination,
            title,
            children,
            ..
        } => {
            out.push('[');
            write_inlines(children, out);
            out.push_str("](");
            out.push_str(destination);
            if let Some(t) = title {
                out.push_str(" \"");
                out.push_str(t);
                out.push('"');
            }
            out.push(')');
        }
        Node::Image {
            destination,
            title,
            alt,
            ..
        } => {
            out.push_str("![");
            out.push_str(alt);
            out.push_str("](");
            out.push_str(destination);
            if let Some(t) = title {
                out.push_str(" \"");
                out.push_str(t);
                out.push('"');
            }
            out.push(')');
        }
        Node::CodeSpan { code, .. } => {
            out.push('`');
            out.push_str(code);
            out.push('`');
        }
        Node::MathInline { math, .. } => {
            out.push('$');
            out.push_str(math);
            out.push('$');
        }
        Node::Html { html, .. } => out.push_str(html),
        Node::SoftBreak { .. } => out.push('\n'),
        Node::HardBreak { .. } => out.push_str("  \n"),
        Node::FootnoteRef { label, .. } => {
            out.push_str("[^");
            out.push_str(label);
            out.push(']');
        }
        // Block-level nodes encountered inline (rare): delegate to write_block.
        other => {
            let mut tmp = String::new();
            write_block(other, &mut tmp, &mut WriteCtx::default());
            out.push_str(&tmp);
        }
    }
}

fn ensure_blank_line(out: &mut String) {
    if !out.ends_with("\n\n") {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::SourceSpan;

    fn s() -> SourceSpan {
        SourceSpan::default()
    }

    #[test]
    fn paragraph_round_trips() {
        let n = Node::<()>::Paragraph {
            children: vec![Node::Text {
                text: "hello".into(),
                source: s(),
            }],
            source: s(),
        };
        assert_eq!(serialize(&n), "hello\n");
    }

    #[test]
    fn heading_emits_atx_form() {
        let n = Node::<()>::Heading {
            level: 2,
            children: vec![Node::Text {
                text: "Title".into(),
                source: s(),
            }],
            source: s(),
        };
        assert_eq!(serialize(&n), "## Title\n");
    }

    #[test]
    fn emphasis_uses_asterisks() {
        let n = Node::<()>::Paragraph {
            children: vec![Node::Emphasis {
                children: vec![Node::Text {
                    text: "x".into(),
                    source: s(),
                }],
                source: s(),
            }],
            source: s(),
        };
        assert_eq!(serialize(&n), "*x*\n");
    }

    #[test]
    fn code_block_uses_fences() {
        let n = Node::<()>::CodeBlock {
            info: Some("rust".into()),
            literal: "let x = 1;\n".into(),
            source: s(),
        };
        assert_eq!(serialize(&n), "```rust\nlet x = 1;\n```\n");
    }

    #[test]
    fn thematic_break() {
        let n = Node::<()>::ThematicBreak { source: s() };
        assert_eq!(serialize(&n), "---\n");
    }

    #[test]
    fn alert_serializes_with_tag() {
        let n = Node::<()>::Alert {
            kind: AlertKind::Warning,
            children: vec![Node::Paragraph {
                children: vec![Node::Text {
                    text: "be careful".into(),
                    source: s(),
                }],
                source: s(),
            }],
            source: s(),
        };
        assert_eq!(serialize(&n), "> [!WARNING]\n> be careful\n");
    }
}
