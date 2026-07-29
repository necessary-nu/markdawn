//! Entry-point shape and the documented round-trip losses of the markdown
//! serializer.

use super::*;
use crate::node::{AlertKind, SourceSpan, TableAlignment};
use crate::parser::ParserBuilder;

fn text(value: &str) -> Node<()> {
    Node::Text {
        text: value.into(),
        source: SourceSpan::default(),
    }
}

fn para(value: &str) -> Node<()> {
    Node::Paragraph {
        children: vec![text(value)],
        source: SourceSpan::default(),
    }
}

fn erase(mut node: Node<()>) -> Node<()> {
    node.walk_mut(&mut |child| *child.source_mut() = SourceSpan::default());
    node
}

fn reparse_with(markdown: &str, builder: ParserBuilder<()>) -> Vec<Node<()>> {
    let mut parser = builder.build();
    parser
        .parse_full_at(markdown, 0)
        .expect("serialized output parses")
        .into_iter()
        .map(|block| erase(block.node))
        .collect()
}

fn reparse(markdown: &str) -> Vec<Node<()>> {
    reparse_with(markdown, ParserBuilder::<()>::new().with_all())
}

// ----- entry points ----------------------------------------------------

// [spec:mw:def:editing.serialize.entry-points/test]
#[test]
fn serialize_takes_one_node_and_serialize_blocks_takes_a_slice() {
    let one = para("alpha");
    assert_eq!(serialize(&one), "alpha\n");

    // An inline node is a legal argument too — `serialize` is not
    // restricted to blocks.
    assert_eq!(serialize(&text("bare")), "bare");

    let blocks = vec![para("alpha"), para("beta")];
    assert_eq!(serialize_blocks(&blocks), "alpha\n\nbeta\n");
    assert_eq!(serialize_blocks::<()>(&[]), "");
    assert_eq!(serialize_blocks(&blocks[..1]), serialize(&blocks[0]));
}

// [spec:mw:def:editing.serialize.entry-points/test]
#[test]
fn every_block_writer_terminates_its_output_with_a_newline() {
    let blocks: Vec<Node<()>> = vec![
        para("p"),
        Node::Heading {
            level: 2,
            children: vec![text("h")],
            source: SourceSpan::default(),
        },
        Node::BlockQuote {
            children: vec![para("q")],
            source: SourceSpan::default(),
        },
        Node::List {
            ordered: false,
            start: None,
            tight: Some(true),
            children: vec![Node::Item {
                children: vec![para("i")],
                source: SourceSpan::default(),
            }],
            source: SourceSpan::default(),
        },
        Node::TaskItem {
            checked: true,
            children: vec![para("t")],
            source: SourceSpan::default(),
        },
        Node::CodeBlock {
            info: Some("rust".into()),
            literal: "x\n".into(),
            source: SourceSpan::default(),
        },
        Node::MathBlock {
            math: "x".into(),
            source: SourceSpan::default(),
        },
        Node::ThematicBreak {
            source: SourceSpan::default(),
        },
        Node::Html {
            html: "<div>d</div>".into(),
            source: SourceSpan::default(),
        },
        Node::Alert {
            kind: AlertKind::Note,
            title: None,
            children: vec![para("a")],
            source: SourceSpan::default(),
        },
        Node::Table {
            alignments: vec![None],
            header: vec![Node::TableRow {
                cells: vec![Node::TableCell {
                    children: vec![text("h")],
                    header: true,
                    alignment: None,
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
            }],
            rows: Vec::new(),
            source: SourceSpan::default(),
        },
        Node::DefinitionList {
            children: vec![
                Node::DefinitionTerm {
                    children: vec![text("term")],
                    source: SourceSpan::default(),
                },
                Node::DefinitionDescription {
                    children: vec![para("def")],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        },
    ];

    for block in &blocks {
        let out = serialize(block);
        assert!(
            out.ends_with('\n'),
            "block writer did not terminate with a newline: {out:?}"
        );
    }
}

// [spec:mw:def:editing.serialize.entry-points/test]
#[test]
fn the_blank_line_separator_is_idempotent_and_never_stacks() {
    // A code block already ends in "\n"; a paragraph ends in "\n"; neither
    // gains more than one blank line from the separator, and a block that
    // already ends in two newlines gains none.
    let blocks = vec![
        Node::CodeBlock {
            info: None,
            literal: "a\n".into(),
            source: SourceSpan::default(),
        },
        para("b"),
        Node::Html {
            html: "<div>c</div>\n\n".into(),
            source: SourceSpan::default(),
        },
        para("d"),
    ];
    let out = serialize_blocks(&blocks);
    assert!(!out.contains("\n\n\n"), "separator stacked: {out:?}");
    assert_eq!(out.matches("\n\n").count(), 3, "{out:?}");

    // Idempotence: separating the same blocks again reproduces the text.
    assert_eq!(serialize_blocks(&blocks), out);
}

// [spec:mw:def:editing.serialize.entry-points/test]
#[test]
fn nesting_is_reconstructed_by_recursion_not_a_running_indent() {
    // The write context is cloned per descent, so a sibling never inherits
    // a previous subtree's depth: the second quote is depth 1 again.
    let blocks = vec![
        Node::BlockQuote {
            children: vec![Node::BlockQuote {
                children: vec![para("deep")],
                source: SourceSpan::default(),
            }],
            source: SourceSpan::default(),
        },
        Node::BlockQuote {
            children: vec![para("shallow")],
            source: SourceSpan::default(),
        },
    ];
    assert_eq!(
        serialize_blocks(&blocks),
        "> > deep\n\n> shallow\n",
        "sibling inherited the previous block's quote depth"
    );

    let nested_list: Node<()> = Node::List {
        ordered: false,
        start: None,
        tight: Some(true),
        children: vec![Node::Item {
            children: vec![
                para("outer"),
                Node::List {
                    ordered: false,
                    start: None,
                    tight: Some(true),
                    children: vec![Node::Item {
                        children: vec![para("inner")],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        }],
        source: SourceSpan::default(),
    };
    // Every continuation line of the item, blank ones included, carries the
    // marker-width indent that the recursive descent reconstructed.
    assert_eq!(serialize(&nested_list), "- outer\n  \n  - inner\n");
}

// [spec:mw:def:editing.serialize.entry-points/test]
#[test]
fn writers_are_pure_functions_of_the_tree_and_ignore_source_spans() {
    let mut spanned = para("hello");
    *spanned.source_mut() = SourceSpan::new(1_000, 2_000);
    spanned.for_each_child_mut(|child| *child.source_mut() = SourceSpan::new(1_234, 1_239));

    assert_eq!(serialize(&spanned), serialize(&para("hello")));
    assert_eq!(
        serialize_blocks(&[spanned]),
        serialize_blocks(&[para("hello")])
    );
}

// ----- documented round-trip losses ------------------------------------

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn only_the_span_erased_shape_round_trips() {
    // Setext + underscore emphasis, so the canonical form differs from the
    // input and every span shifts.
    let source = "Title\n=====\n\nSome _emphasis_ here.\n";
    let original = reparse(source);
    let markdown = serialize_blocks(&original);
    let reparsed = reparse(&markdown);

    // Span-erased shapes agree...
    assert_eq!(
        reparsed,
        original.iter().cloned().map(erase).collect::<Vec<_>>()
    );

    // ...but the spans themselves are recomputed against the serialised
    // text, so full `Node` equality (which includes `source`) does not hold.
    let mut parser = ParserBuilder::<()>::new().with_all().build();
    let spanned: Vec<Node<()>> = parser
        .parse_full_at(source, 0)
        .expect("parse")
        .into_iter()
        .map(|block| block.node)
        .collect();
    let respanned: Vec<Node<()>> = parser
        .parse_full_at(&markdown, 0)
        .expect("parse")
        .into_iter()
        .map(|block| block.node)
        .collect();
    assert_ne!(
        respanned, spanned,
        "spans happened to survive; the rule's warning would be stale"
    );
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn tight_lists_come_back_loose() {
    let tight = reparse("- a\n- b\n");
    assert!(
        matches!(
            &tight[0],
            Node::List {
                tight: Some(true),
                ..
            }
        ),
        "{tight:?}"
    );

    let loose = reparse(&serialize_blocks(&tight));
    assert!(
        matches!(&loose[0], Node::List { tight, .. } if *tight != Some(true)),
        "tightness survived: {loose:?}"
    );
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn footnote_definitions_come_back_as_prose_with_a_dangling_reference() {
    let original = reparse("see[^a]\n\n[^a]: the note\n");
    assert!(
        original.iter().any(|block| {
            let mut found = false;
            block.walk(&mut |node| {
                found |= matches!(node, Node::FootnoteDefinition { .. });
            });
            found
        }),
        "{original:?}"
    );

    let markdown = serialize_blocks(&original);
    let reparsed = reparse(&markdown);

    let mut definitions = 0usize;
    let mut refs = 0usize;
    let mut literal = String::new();
    for block in &reparsed {
        block.walk(&mut |node| match node {
            Node::FootnoteDefinition { .. } | Node::FootnoteSection { .. } => definitions += 1,
            Node::FootnoteRef { .. } => refs += 1,
            Node::Text { text, .. } => literal.push_str(text),
            _ => {}
        });
    }
    assert_eq!(definitions, 0, "definition survived: {markdown:?}");
    // The label is dropped, so the emitted `[^a]` no longer resolves: it
    // comes back as literal text rather than a reference.
    assert_eq!(refs, 0, "reference resolved against nothing: {markdown:?}");
    assert!(literal.contains("[^a]"), "{literal:?} from {markdown:?}");
    // The definition's children are emitted as bare blocks.
    assert!(
        markdown.contains("the note") && !markdown.contains("[^a]: "),
        "children should be emitted as bare blocks: {markdown:?}"
    );
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn sub_and_superscript_survive_at_the_top_level_but_not_inside_another_inline() {
    // Collect the promoted / unpromoted inline kinds after a cycle.
    let cycle = |source: &str| -> Vec<&'static str> {
        let markdown = serialize_blocks(&reparse(source));
        let mut kinds = Vec::new();
        for block in &reparse(&markdown) {
            block.walk(&mut |node| match node {
                Node::Subscript { .. } => kinds.push("sub"),
                Node::Superscript { .. } => kinds.push("sup"),
                Node::Html { .. } => kinds.push("html"),
                _ => {}
            });
        }
        kinds
    };

    // Directly in a block's inline run the `<sub>` / `<sup>` the writer
    // emits are promoted back, so the node survives.
    assert_eq!(cycle("H<sub>2</sub>O\n"), vec!["sub"]);
    assert_eq!(cycle("x<sup>2</sup>\n"), vec!["sup"]);
    assert_eq!(cycle("<sub>a <sup>b</sup></sub>\n"), vec!["sub", "sup"]);

    // Nested inside another inline the promotion does not reach them, and
    // they come back as the open tag, the content, and the close tag.
    for source in [
        "*<sub>2</sub>*\n",
        "**<sub>2</sub>**\n",
        "~~<sub>2</sub>~~\n",
        "==<sub>2</sub>==\n",
        "[<sub>2</sub>](https://example.com)\n",
    ] {
        assert_eq!(cycle(source), vec!["html", "html"], "{source:?}");
    }
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn a_task_item_in_an_ordered_list_comes_back_unordered() {
    let original: Node<()> = Node::List {
        ordered: true,
        start: Some(1),
        tight: Some(false),
        children: vec![Node::TaskItem {
            checked: false,
            children: vec![para("todo")],
            source: SourceSpan::default(),
        }],
        source: SourceSpan::default(),
    };

    let markdown = serialize(&original);
    assert!(markdown.starts_with("- [ ] "), "{markdown:?}");
    let reparsed = reparse(&markdown);
    assert!(
        matches!(&reparsed[0], Node::List { ordered: false, .. }),
        "{reparsed:?}"
    );
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn table_cell_header_and_alignment_are_rebuilt_only_from_the_table() {
    // A cell whose own `alignment` disagrees with the table's `alignments`
    // list: the cell field is dropped, so the list is what comes back.
    let cell = |value: &str, header: bool, alignment: Option<TableAlignment>| -> Node<()> {
        Node::TableCell {
            children: vec![text(value)],
            header,
            alignment,
            source: SourceSpan::default(),
        }
    };
    let original: Node<()> = Node::Table {
        alignments: vec![Some(TableAlignment::Right)],
        header: vec![Node::TableRow {
            cells: vec![cell("h", true, Some(TableAlignment::Left))],
            source: SourceSpan::default(),
        }],
        rows: vec![Node::TableRow {
            cells: vec![cell("b", false, Some(TableAlignment::Center))],
            source: SourceSpan::default(),
        }],
        source: SourceSpan::default(),
    };

    let reparsed = reparse(&serialize(&original));
    let Node::Table {
        alignments,
        header,
        rows,
        ..
    } = &reparsed[0]
    else {
        panic!("expected a table, got {reparsed:?}");
    };
    assert_eq!(alignments, &vec![Some(TableAlignment::Right)]);

    let alignment_of = |row: &Node<()>| -> Option<TableAlignment> {
        let Node::TableRow { cells, .. } = row else {
            panic!("expected a row");
        };
        match &cells[0] {
            Node::TableCell { alignment, .. } => *alignment,
            other => panic!("expected a cell, got {other:?}"),
        }
    };
    // Both cells pick up the table's column alignment, not their own.
    assert_eq!(alignment_of(&header[0]), Some(TableAlignment::Right));
    assert_eq!(alignment_of(&rows[0]), Some(TableAlignment::Right));

    // `header` is reconstructed from position, not carried across.
    let header_flag = |row: &Node<()>| -> bool {
        let Node::TableRow { cells, .. } = row else {
            panic!("expected a row");
        };
        matches!(&cells[0], Node::TableCell { header: true, .. })
    };
    assert!(header_flag(&header[0]));
    assert!(!header_flag(&rows[0]));
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn heading_levels_are_clamped_to_six() {
    let deep: Node<()> = Node::Heading {
        level: 9,
        children: vec![text("too deep")],
        source: SourceSpan::default(),
    };
    assert_eq!(serialize(&deep), "###### too deep\n");

    let reparsed = reparse(&serialize(&deep));
    assert!(
        matches!(&reparsed[0], Node::Heading { level: 6, .. }),
        "{reparsed:?}"
    );
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn serialisation_normalises_markers_indentation_spacing_and_numbering() {
    let source = "Setext\n======\n\n\
         \x20   indented code\n\n\
         _under_ emphasis and __under__ strong\n\n\
         * star bullet\n+ plus bullet\n\n\
         5. five\n9. nine\n\n\
         >lazy\n>quote\n";
    let markdown = serialize_blocks(&reparse(source));

    assert!(markdown.contains("# Setext\n"), "{markdown}");
    assert!(markdown.contains("*under* emphasis"), "{markdown}");
    assert!(markdown.contains("**under** strong"), "{markdown}");
    assert!(!markdown.contains('_'), "{markdown}");
    assert!(markdown.contains("- star bullet"), "{markdown}");
    assert!(!markdown.contains("* star"), "{markdown}");
    assert!(!markdown.contains("+ plus"), "{markdown}");
    // Ordered markers renumbered consecutively from `start`.
    assert!(markdown.contains("5. five"), "{markdown}");
    assert!(markdown.contains("6. nine"), "{markdown}");
    // Code always fenced rather than indented.
    assert!(markdown.contains("```\nindented code\n```"), "{markdown}");
    // Block quotes re-prefixed line by line.
    assert!(markdown.contains("> lazy\n> quote"), "{markdown}");
    // Blocks re-separated by exactly one blank line.
    assert!(!markdown.contains("\n\n\n"), "{markdown}");
}

// [spec:mw:req:editing.serialize.round-trip+2/test]
#[test]
fn fidelity_is_conditional_on_the_reparsing_parser_having_the_same_extensions() {
    let alert: Node<()> = Node::Alert {
        kind: AlertKind::Note,
        title: None,
        children: vec![para("body")],
        source: SourceSpan::default(),
    };
    let markdown = serialize(&alert);
    assert!(markdown.contains("[!NOTE]"), "{markdown}");

    // With alerts enabled the alert comes back...
    let with = reparse(&markdown);
    assert!(matches!(&with[0], Node::Alert { .. }), "{with:?}");
    // ...and with alerts off it degrades to a plain block quote.
    let without = reparse_with(&markdown, ParserBuilder::<()>::new().with_gfm());
    assert!(
        matches!(&without[0], Node::BlockQuote { .. }),
        "{without:?}"
    );

    let struck: Node<()> = Node::Paragraph {
        children: vec![Node::Strikethrough {
            children: vec![text("x")],
            source: SourceSpan::default(),
        }],
        source: SourceSpan::default(),
    };
    let markdown = serialize(&struck);
    assert_eq!(markdown, "~~x~~\n");
    let without_gfm = reparse_with(&markdown, ParserBuilder::<()>::new());
    let mut struck_nodes = 0usize;
    without_gfm[0].walk(&mut |node| {
        struck_nodes += usize::from(matches!(node, Node::Strikethrough { .. }));
    });
    assert_eq!(
        struck_nodes, 0,
        "strikethrough survived a non-GFM reparse: {without_gfm:?}"
    );
}
