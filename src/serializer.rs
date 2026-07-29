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

use crate::node::Node;

#[path = "serializer_block.rs"]
mod block;

use block::write_block;

/// Serialize a single node (block or inline) to canonical markdown.
// [spec:mw:def:editing.serialize.entry-points]
// [spec:mw:req:editing.serialize.round-trip+1]
pub fn serialize<Ext>(node: &Node<Ext>) -> String {
    let mut out = String::new();
    write_block(node, &mut out, &mut WriteCtx::default());
    out
}

/// Serialize a sequence of top-level blocks to canonical markdown, joined
/// with blank-line separators.
// [spec:mw:def:editing.serialize.entry-points]
// [spec:mw:req:editing.serialize.round-trip+1]
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

/// Inline emission state: whether the next character written begins a fresh
/// output line, where a leading marker would be read as a block construct,
/// and whether emission sits inside a bracketed link or image label, where a
/// closing bracket ends the label.
#[derive(Clone, Copy)]
struct InlineCtx {
    line_start: bool,
    label: bool,
}

impl InlineCtx {
    /// Inlines placed at the head of a block's own line.
    const fn flow() -> Self {
        Self {
            line_start: true,
            label: false,
        }
    }

    /// Inlines placed after text that already occupies the line, or inside a
    /// container where block markers cannot begin.
    const fn mid_line() -> Self {
        Self {
            line_start: false,
            label: false,
        }
    }
}

fn write_inlines<Ext>(nodes: &[Node<Ext>], out: &mut String, ctx: &mut InlineCtx) {
    for node in nodes {
        write_inline(node, out, ctx);
    }
}

// [spec:mw:sem:editing.serialize.inline-forms+1]
// [spec:mw:req:editing.serialize.escaping+1]
fn write_inline<Ext>(node: &Node<Ext>, out: &mut String, ctx: &mut InlineCtx) {
    let line_start = ctx.line_start;
    ctx.line_start = false;
    match node {
        Node::Text { text, .. } => {
            ctx.line_start = write_escaped_text(text, line_start, ctx.label, out);
        }
        Node::Emphasis { children, .. } => {
            out.push('*');
            write_inlines(children, out, ctx);
            out.push('*');
        }
        Node::Strong { children, .. } => {
            out.push_str("**");
            write_inlines(children, out, ctx);
            out.push_str("**");
        }
        Node::Strikethrough { children, .. } => {
            out.push_str("~~");
            write_inlines(children, out, ctx);
            out.push_str("~~");
        }
        Node::Subscript { children, .. } => {
            out.push_str("<sub>");
            write_inlines(children, out, ctx);
            out.push_str("</sub>");
        }
        Node::Superscript { children, .. } => {
            out.push_str("<sup>");
            write_inlines(children, out, ctx);
            out.push_str("</sup>");
        }
        Node::Highlight { children, .. } => {
            out.push_str("==");
            write_inlines(children, out, ctx);
            out.push_str("==");
        }
        Node::Link {
            destination,
            title,
            children,
            ..
        } => {
            out.push('[');
            let outer = ctx.label;
            ctx.label = true;
            write_inlines(children, out, ctx);
            ctx.label = outer;
            out.push(']');
            write_link_target(destination, title.as_deref(), out);
        }
        Node::Image {
            destination,
            title,
            alt,
            ..
        } => {
            out.push_str("![");
            write_escaped_text(alt, false, true, out);
            out.push(']');
            write_link_target(destination, title.as_deref(), out);
        }
        Node::CodeSpan { code, .. } => write_code_span(code, out),
        Node::MathInline { math, .. } => {
            out.push('$');
            out.push_str(math);
            out.push('$');
        }
        Node::Html { html, .. } => out.push_str(html),
        Node::SoftBreak { .. } => {
            out.push('\n');
            ctx.line_start = true;
        }
        Node::HardBreak { .. } => {
            out.push_str("  \n");
            ctx.line_start = true;
        }
        Node::FootnoteRef { label, .. } => {
            out.push_str("[^");
            out.push_str(label);
            out.push(']');
        }
        Node::TableCell { children, .. } => write_inlines(children, out, ctx),
        Node::Custom { children, .. } => write_inlines(children, out, ctx),
        // Block-level nodes encountered inline (rare): delegate to write_block,
        // which matches every one of them without delegating back.
        Node::Document { .. }
        | Node::Paragraph { .. }
        | Node::Heading { .. }
        | Node::BlockQuote { .. }
        | Node::Alert { .. }
        | Node::List { .. }
        | Node::Item { .. }
        | Node::TaskItem { .. }
        | Node::Table { .. }
        | Node::TableRow { .. }
        | Node::CodeBlock { .. }
        | Node::MathBlock { .. }
        | Node::ThematicBreak { .. }
        | Node::FootnoteDefinition { .. }
        | Node::FootnoteSection { .. }
        | Node::DefinitionList { .. }
        | Node::DefinitionTerm { .. }
        | Node::DefinitionDescription { .. } => {
            let mut tmp = String::new();
            write_block(node, &mut tmp, &mut WriteCtx::default());
            out.push_str(&tmp);
        }
    }
}

/// The code span's delimiter is widened past the longest backtick run the
/// content holds, and the content is padded when it touches a backtick so
/// the opening and closing runs stay exactly the chosen length.
// [spec:mw:req:editing.serialize.escaping+1]
fn write_code_span(code: &str, out: &mut String) {
    let code = code.replace(['\n', '\r'], " ");
    let fence = shortest_free_backtick_run(&code);
    let pad = code.starts_with('`') || code.ends_with('`');
    for _ in 0..fence {
        out.push('`');
    }
    if pad {
        out.push(' ');
    }
    out.push_str(&code);
    if pad {
        out.push(' ');
    }
    for _ in 0..fence {
        out.push('`');
    }
}

fn shortest_free_backtick_run(code: &str) -> usize {
    let mut runs = Vec::new();
    let mut run = 0usize;
    for ch in code.chars() {
        if ch == '`' {
            run += 1;
        } else {
            if run > 0 {
                runs.push(run);
            }
            run = 0;
        }
    }
    if run > 0 {
        runs.push(run);
    }
    let ceiling = runs.iter().copied().max().unwrap_or(0) + 1;
    (1..=ceiling).find(|n| !runs.contains(n)).unwrap_or(1)
}

// [spec:mw:req:editing.serialize.escaping+1]
fn write_link_target(destination: &str, title: Option<&str>, out: &mut String) {
    out.push('(');
    write_destination(destination, out);
    if let Some(title) = title {
        out.push_str(" \"");
        write_title(title, out);
        out.push('"');
    }
    out.push(')');
}

fn write_destination(destination: &str, out: &mut String) {
    let angle = destination.is_empty()
        || destination
            .chars()
            .any(|ch| ch.is_ascii_whitespace() || ch == '<' || ch == '>')
        || !parens_balanced(destination);
    if angle {
        out.push('<');
    }
    let mut index = 0;
    while index < destination.len() {
        let rest = &destination[index..];
        let ch = rest.chars().next().unwrap_or('\0');
        let escape = match ch {
            '\\' => true,
            '<' | '>' => angle,
            '&' => starts_entity(rest),
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(ch);
        index += ch.len_utf8();
    }
    if angle {
        out.push('>');
    }
}

fn write_title(title: &str, out: &mut String) {
    let mut index = 0;
    while index < title.len() {
        let rest = &title[index..];
        let ch = rest.chars().next().unwrap_or('\0');
        let escape = match ch {
            '\\' | '"' | '(' | ')' => true,
            '&' => starts_entity(rest),
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        if ch == '\n' || ch == '\r' {
            out.push(' ');
        } else {
            out.push(ch);
        }
        index += ch.len_utf8();
    }
}

fn parens_balanced(text: &str) -> bool {
    let mut depth = 0i32;
    for ch in text.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

enum Escape {
    /// Emit the run as it stands.
    None,
    /// Backslash the first character only; the rest of the run is inert once
    /// the construct it would open has been broken.
    First,
    /// Backslash every character of the run.
    Run,
}

/// Characters whose meaning depends on how many of them appear in a row.
fn is_run_sensitive(ch: char) -> bool {
    matches!(
        ch,
        '`' | '*' | '_' | '~' | '=' | '$' | '#' | '-' | '>' | '\\'
    )
}

/// Escape the markup-significant characters of a text payload, and report
/// whether the emitted text left the cursor at the start of a line.
// [spec:mw:req:editing.serialize.escaping+1]
fn write_escaped_text(text: &str, mut line_start: bool, label: bool, out: &mut String) -> bool {
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if line_start && let Some(width) = write_ordered_marker(rest, out) {
            index += width;
            line_start = false;
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\0');
        if ch == '\n' {
            out.push('\n');
            index += 1;
            line_start = true;
            continue;
        }
        let run = if is_run_sensitive(ch) {
            rest.chars().take_while(|next| *next == ch).count()
        } else {
            1
        };
        let width = ch.len_utf8() * run;
        let previous = text[..index].chars().next_back();
        match text_escape(ch, run, previous, rest, line_start, label) {
            Escape::None => out.push_str(&rest[..width]),
            Escape::First => {
                out.push('\\');
                out.push_str(&rest[..width]);
            }
            Escape::Run => {
                for _ in 0..run {
                    out.push('\\');
                    out.push(ch);
                }
            }
        }
        index += width;
        line_start = false;
    }
    line_start
}

fn text_escape(
    ch: char,
    run: usize,
    previous: Option<char>,
    rest: &str,
    line_start: bool,
    label: bool,
) -> Escape {
    let after = &rest[ch.len_utf8() * run..];
    let escapes = match ch {
        '\\' | '`' | '*' | '[' => true,
        // An unmatched closing bracket is inert; inside a label it ends the
        // label early.
        ']' => label,
        // An underscore run flanked by alphanumerics on both sides can
        // neither open nor close emphasis.
        '_' => {
            !(previous.is_some_and(char::is_alphanumeric)
                && after.chars().next().is_some_and(char::is_alphanumeric))
        }
        // Every construct an angle bracket opens needs a later `>`.
        '<' => {
            after.contains('>')
                && after.starts_with(|next: char| {
                    next.is_ascii_alphanumeric() || next == '/' || next == '!' || next == '?'
                })
        }
        '&' => starts_entity(rest),
        // Three or more tildes are literal inline, but open a fence at the
        // start of a line.
        '~' => run <= 2 || line_start,
        '=' => run >= 2 || (line_start && line_tail_blank(after)),
        '$' => run >= 2 || after.contains('$'),
        '#' => line_start && run <= 6 && starts_block_gap(after),
        '>' => line_start,
        '-' => line_start && (run >= 3 || starts_block_gap(after)),
        '+' => line_start && starts_block_gap(after),
        ':' => line_start && after.starts_with([' ', '\t']),
        _ => false,
    };
    if !escapes {
        Escape::None
    } else if matches!(ch, '#' | '>' | '-' | '+' | ':') {
        Escape::First
    } else {
        Escape::Run
    }
}

/// Break an ordered-list marker at the start of a line by escaping its
/// delimiter, leaving the digits alone.
fn write_ordered_marker(rest: &str, out: &mut String) -> Option<usize> {
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let delimiter = rest[digits..].chars().next()?;
    if delimiter != '.' && delimiter != ')' {
        return None;
    }
    if !starts_block_gap(&rest[digits + 1..]) {
        return None;
    }
    out.push_str(&rest[..digits]);
    out.push('\\');
    out.push(delimiter);
    Some(digits + 1)
}

fn starts_block_gap(after: &str) -> bool {
    after.is_empty() || after.starts_with([' ', '\t', '\n'])
}

fn line_tail_blank(after: &str) -> bool {
    after.split('\n').next().unwrap_or("").trim().is_empty()
}

/// Whether an ampersand opens something the parser would decode as an
/// entity or numeric character reference.
fn starts_entity(rest: &str) -> bool {
    let body = &rest[1..];
    let (body, digits_only, hex) = match body.strip_prefix('#') {
        Some(numeric) => match numeric.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, true, true),
            None => (numeric, true, false),
        },
        None => (body, false, false),
    };
    let Some(end) = body.find(';') else {
        return false;
    };
    if end == 0 || end > 32 {
        return false;
    }
    body[..end].chars().all(|ch| {
        if hex {
            ch.is_ascii_hexdigit()
        } else if digits_only {
            ch.is_ascii_digit()
        } else {
            ch.is_ascii_alphanumeric()
        }
    })
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
    use crate::node::{AlertKind, SourceSpan, TableAlignment};
    use crate::parser::ParserBuilder;

    fn s() -> SourceSpan {
        SourceSpan::default()
    }

    fn text(value: &str) -> Node<()> {
        Node::Text {
            text: value.into(),
            source: s(),
        }
    }

    fn paragraph(children: Vec<Node<()>>) -> Node<()> {
        Node::Paragraph {
            children,
            source: s(),
        }
    }

    fn erase(mut node: Node<()>) -> Node<()> {
        node.walk_mut(&mut |child| *child.source_mut() = SourceSpan::default());
        node
    }

    fn reparse(markdown: &str) -> Vec<Node<()>> {
        let mut parser = ParserBuilder::<()>::new().with_all().build();
        parser
            .parse_full_at(markdown, 0)
            .expect("serialized output parses")
            .into_iter()
            .map(|block| erase(block.node))
            .collect()
    }

    /// Serialise, re-parse, and compare span-erased shapes: `SourceSpan`
    /// values are recomputed against the serialised text, so only the shape
    /// can be compared.
    #[track_caller]
    fn assert_round_trips(blocks: &[Node<()>]) {
        let markdown = serialize_blocks(blocks);
        let expected: Vec<Node<()>> = blocks.iter().cloned().map(erase).collect();
        assert_eq!(reparse(&markdown), expected, "serialized to:\n{markdown}");
    }

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
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

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
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

    // [spec:mw:sem:editing.serialize.inline-forms+1/test]
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

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
    #[test]
    fn code_block_uses_fences() {
        let n = Node::<()>::CodeBlock {
            info: Some("rust".into()),
            literal: "let x = 1;\n".into(),
            source: s(),
        };
        assert_eq!(serialize(&n), "```rust\nlet x = 1;\n```\n");
    }

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
    #[test]
    fn thematic_break() {
        let n = Node::<()>::ThematicBreak { source: s() };
        assert_eq!(serialize(&n), "---\n");
    }

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
    // [spec:mw:def:core.node.alert-kinds/test]
    #[test]
    fn alert_serializes_with_tag() {
        let n = Node::<()>::Alert {
            kind: AlertKind::Warning,
            title: None,
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

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
    #[test]
    fn alert_with_custom_title_serializes_title_on_marker_line() {
        let n = Node::<()>::Alert {
            kind: AlertKind::Note,
            title: Some("Custom title".into()),
            children: vec![Node::Paragraph {
                children: vec![Node::Text {
                    text: "body".into(),
                    source: s(),
                }],
                source: s(),
            }],
            source: s(),
        };
        assert_eq!(serialize(&n), "> [!NOTE] Custom title\n> body\n");
    }

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
    #[test]
    fn table_row_serializes_standalone() {
        let n = Node::<()>::TableRow {
            cells: vec![
                Node::TableCell {
                    children: vec![text("a")],
                    header: false,
                    alignment: None,
                    source: s(),
                },
                Node::TableCell {
                    children: vec![text("b")],
                    header: false,
                    alignment: None,
                    source: s(),
                },
            ],
            source: s(),
        };
        assert_eq!(serialize(&n), "| a | b |\n");
    }

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
    #[test]
    fn table_cell_serializes_standalone() {
        let n = Node::<()>::TableCell {
            children: vec![text("solo")],
            header: true,
            alignment: Some(TableAlignment::Center),
            source: s(),
        };
        assert_eq!(serialize(&n), "solo\n");
    }

    // [spec:mw:sem:editing.serialize.block-forms+1/test]
    #[test]
    fn custom_serializes_children() {
        let n = Node::<()>::Custom {
            payload: (),
            children: vec![paragraph(vec![text("inner")])],
            source: s(),
        };
        assert_eq!(serialize(&n), "inner\n");
    }

    // [spec:mw:sem:editing.serialize.inline-forms+1/test]
    #[test]
    fn custom_inside_a_paragraph_serializes_children_inline() {
        let n = paragraph(vec![
            text("a"),
            Node::Custom {
                payload: (),
                children: vec![text("b")],
                source: s(),
            },
            text("c"),
        ]);
        assert_eq!(serialize(&n), "abc\n");
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn inline_markup_characters_survive_a_round_trip() {
        assert_round_trips(&[paragraph(vec![text("a*b_c`d[e]f<g>h~i==j$k$l&amp;m")])]);
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn block_markers_are_escaped_only_at_the_start_of_a_line() {
        assert_round_trips(&[paragraph(vec![
            text("# not a heading"),
            Node::SoftBreak { source: s() },
            text("> not a quote"),
            Node::SoftBreak { source: s() },
            text("- not a bullet"),
            Node::SoftBreak { source: s() },
            text("1. not an item"),
            Node::SoftBreak { source: s() },
            text("--- not a break"),
            Node::SoftBreak { source: s() },
            text("mid-line # > - 1. is plain"),
        ])]);
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn escaping_stays_minimal() {
        let n = paragraph(vec![text(
            "snake_case_name costs $5 today (a<b) 3.14 e.g. a-b",
        )]);
        assert_eq!(
            serialize(&n),
            "snake_case_name costs $5 today (a<b) 3.14 e.g. a-b\n"
        );
        assert_round_trips(std::slice::from_ref(&n));
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn code_span_widens_its_backtick_fence() {
        for code in ["a ` b", "``", "`edge`", "a `` b ` c"] {
            assert_round_trips(&[paragraph(vec![Node::CodeSpan {
                code: code.into(),
                source: s(),
            }])]);
        }
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn code_block_widens_its_fence_past_a_closing_fence_line() {
        assert_round_trips(&[Node::CodeBlock {
            info: Some("rust".into()),
            literal: "before\n```\nafter\n".into(),
            source: s(),
        }]);
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn code_block_switches_to_tildes_for_a_backticked_info_string() {
        assert_round_trips(&[Node::CodeBlock {
            info: Some("run `cmd`".into()),
            literal: "body\n".into(),
            source: s(),
        }]);
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn table_cell_pipes_are_escaped() {
        let cell = |value: &str, header: bool| Node::TableCell {
            children: vec![text(value)],
            header,
            alignment: None,
            source: s(),
        };
        assert_round_trips(&[Node::Table {
            alignments: vec![None, None],
            header: vec![Node::TableRow {
                cells: vec![cell("a|b", true), cell("c", true)],
                source: s(),
            }],
            rows: vec![Node::TableRow {
                cells: vec![
                    Node::TableCell {
                        children: vec![Node::CodeSpan {
                            code: "x|y".into(),
                            source: s(),
                        }],
                        header: false,
                        alignment: None,
                        source: s(),
                    },
                    cell("d", false),
                ],
                source: s(),
            }],
            source: s(),
        }]);
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn link_titles_and_destinations_are_escaped() {
        assert_round_trips(&[paragraph(vec![
            Node::Link {
                destination: "/a b".into(),
                title: Some("say \"hi\" (now)".into()),
                children: vec![text("one")],
                source: s(),
            },
            text(" "),
            Node::Link {
                destination: "/wiki/Foo_(bar)".into(),
                title: None,
                children: vec![text("two")],
                source: s(),
            },
            text(" "),
            Node::Link {
                destination: String::new(),
                title: Some("empty".into()),
                children: vec![text("three")],
                source: s(),
            },
        ])]);
    }

    // [spec:mw:req:editing.serialize.escaping+1/test]
    #[test]
    fn image_alt_text_is_escaped() {
        assert_round_trips(&[paragraph(vec![Node::Image {
            destination: "/u".into(),
            title: None,
            alt: "a]b*c".into(),
            source: s(),
        }])]);
    }
}
