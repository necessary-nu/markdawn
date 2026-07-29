//! Tests for the generic node tree: variant coverage, the total `source`
//! accessors, and the traversal helpers.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Payload(&'static str);

type N = Node<Payload>;

/// A container child that itself has a child, so a traversal helper that
/// wrongly recursed would be visible as an extra visit.
fn marker() -> N {
    Node::Paragraph {
        children: vec![Node::Text {
            text: "leaf".to_owned(),
            source: SourceSpan::new(0, 4),
        }],
        source: SourceSpan::new(0, 4),
    }
}

fn span_for(index: usize) -> SourceSpan {
    SourceSpan::new(index * 10, index * 10 + 5)
}

/// Every `Node` variant, in declaration order, each carrying a distinct
/// span and — for the container variants — exactly one [`marker`] child.
/// `Table` gets one header row and one body row.
fn all_variants() -> Vec<N> {
    let mut index = 0;
    let mut next = || {
        let span = span_for(index);
        index += 1;
        span
    };
    vec![
        Node::Document {
            children: vec![marker()],
            source: next(),
        },
        Node::Paragraph {
            children: vec![marker()],
            source: next(),
        },
        Node::Heading {
            level: 2,
            children: vec![marker()],
            source: next(),
        },
        Node::BlockQuote {
            children: vec![marker()],
            source: next(),
        },
        Node::List {
            ordered: true,
            start: Some(3),
            tight: Some(true),
            children: vec![marker()],
            source: next(),
        },
        Node::Item {
            children: vec![marker()],
            source: next(),
        },
        Node::TaskItem {
            checked: true,
            children: vec![marker()],
            source: next(),
        },
        Node::Table {
            alignments: vec![Some(TableAlignment::Left)],
            header: vec![marker()],
            rows: vec![marker()],
            source: next(),
        },
        Node::TableRow {
            cells: vec![marker()],
            source: next(),
        },
        Node::TableCell {
            children: vec![marker()],
            header: true,
            alignment: Some(TableAlignment::Center),
            source: next(),
        },
        Node::Text {
            text: "t".to_owned(),
            source: next(),
        },
        Node::Emphasis {
            children: vec![marker()],
            source: next(),
        },
        Node::Strong {
            children: vec![marker()],
            source: next(),
        },
        Node::Strikethrough {
            children: vec![marker()],
            source: next(),
        },
        Node::Subscript {
            children: vec![marker()],
            source: next(),
        },
        Node::Superscript {
            children: vec![marker()],
            source: next(),
        },
        Node::Highlight {
            children: vec![marker()],
            source: next(),
        },
        Node::Link {
            destination: "/d".to_owned(),
            title: Some("ti".to_owned()),
            children: vec![marker()],
            source: next(),
        },
        Node::Image {
            destination: "/i".to_owned(),
            title: None,
            alt: "alt".to_owned(),
            source: next(),
        },
        Node::CodeSpan {
            code: "c".to_owned(),
            source: next(),
        },
        Node::CodeBlock {
            info: Some("rust".to_owned()),
            literal: "fn main() {}\n".to_owned(),
            source: next(),
        },
        Node::MathInline {
            math: "x".to_owned(),
            source: next(),
        },
        Node::MathBlock {
            math: "y".to_owned(),
            source: next(),
        },
        Node::Html {
            html: "<b>".to_owned(),
            source: next(),
        },
        Node::SoftBreak { source: next() },
        Node::HardBreak { source: next() },
        Node::ThematicBreak { source: next() },
        Node::FootnoteRef {
            label: "a".to_owned(),
            number: 1,
            ref_num: 1,
            source: next(),
        },
        Node::FootnoteDefinition {
            label: "a".to_owned(),
            number: 1,
            backref_count: 1,
            children: vec![marker()],
            source: next(),
        },
        Node::FootnoteSection {
            children: vec![marker()],
            source: next(),
        },
        Node::Alert {
            kind: AlertKind::Warning,
            title: None,
            children: vec![marker()],
            source: next(),
        },
        Node::DefinitionList {
            children: vec![marker()],
            source: next(),
        },
        Node::DefinitionTerm {
            children: vec![marker()],
            source: next(),
        },
        Node::DefinitionDescription {
            children: vec![marker()],
            source: next(),
        },
        Node::Custom {
            payload: Payload("p"),
            children: vec![marker()],
            source: next(),
        },
    ]
}

/// The eleven leaf variants, identified by the shape `for_each_child`
/// promises yields nothing.
fn is_leaf(node: &N) -> bool {
    matches!(
        node,
        Node::Text { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::CodeBlock { .. }
            | Node::MathInline { .. }
            | Node::MathBlock { .. }
            | Node::Html { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::ThematicBreak { .. }
            | Node::FootnoteRef { .. }
    )
}

// [spec:mw:def:core.node.generic-tree/test]
#[test]
fn one_generic_enum_covers_thirty_five_variants_with_eleven_leaves() {
    let variants = all_variants();
    assert_eq!(variants.len(), 35);
    assert_eq!(variants.iter().filter(|node| is_leaf(node)).count(), 11);
    // Block constructs (Paragraph) and inline constructs (Emphasis) are
    // the same type, so both fit in one `Vec<Node<Payload>>`, and the
    // extension payload is an arbitrary caller type rather than `()`.
    assert!(matches!(variants[1], Node::Paragraph { .. }));
    assert!(matches!(variants[11], Node::Emphasis { .. }));
    assert!(matches!(
        &variants[34],
        Node::Custom { payload, .. } if payload == &Payload("p")
    ));
}

// [spec:mw:def:core.node.generic-tree/test]
#[test]
fn node_derives_clone_eq_and_debug_over_every_variant() {
    for node in all_variants() {
        let cloned = node.clone();
        assert_eq!(cloned, node);
        assert!(!format!("{node:?}").is_empty());

        // Inequality is structural, not identity-based: mutating the
        // clone's span makes it differ from the original.
        let mut mutated = node.clone();
        *mutated.source_mut() = SourceSpan::new(9_999, 10_000);
        assert_ne!(mutated, node);
    }
}

// [spec:mw:req:core.node.source-accessors/test]
#[test]
fn source_accessors_are_total_over_every_variant() {
    for (index, node) in all_variants().into_iter().enumerate() {
        let mut node = node;
        assert_eq!(
            node.source(),
            span_for(index),
            "source() wrong for variant {index}"
        );

        let replacement = SourceSpan::new(1_000 + index, 1_100 + index);
        *node.source_mut() = replacement;
        assert_eq!(
            node.source(),
            replacement,
            "source_mut() did not write through for variant {index}"
        );
    }
}

// [spec:mw:req:core.traversal.direct-children/test]
#[test]
fn for_each_child_visits_direct_children_only() {
    for (index, node) in all_variants().into_iter().enumerate() {
        let mut visited = 0usize;
        node.for_each_child(|_| visited += 1);

        let expected = match &node {
            _ if is_leaf(&node) => 0,
            Node::Table { .. } => 2,
            _ => 1,
        };
        assert_eq!(
            visited, expected,
            "variant {index} yielded {visited} children, expected {expected}"
        );
    }
}

// [spec:mw:req:core.traversal.direct-children/test]
#[test]
fn for_each_child_yields_document_order_and_does_not_recurse() {
    let tree: N = Node::Document {
        children: vec![
            Node::Text {
                text: "first".to_owned(),
                source: SourceSpan::new(0, 5),
            },
            Node::Emphasis {
                children: vec![Node::Text {
                    text: "nested".to_owned(),
                    source: SourceSpan::new(6, 12),
                }],
                source: SourceSpan::new(5, 13),
            },
            Node::Text {
                text: "last".to_owned(),
                source: SourceSpan::new(13, 17),
            },
        ],
        source: SourceSpan::new(0, 17),
    };

    let mut seen = Vec::new();
    tree.for_each_child(|child| seen.push(child.source().start));
    // The `Emphasis` child's own `Text` (start 6) is absent: no recursion.
    assert_eq!(seen, vec![0, 5, 13]);
}

// [spec:mw:sem:core.traversal.table-child-order/test]
#[test]
fn table_yields_header_rows_before_body_rows_not_cells() {
    let row = |start: usize, cell_start: usize| -> N {
        Node::TableRow {
            cells: vec![Node::TableCell {
                children: Vec::new(),
                header: false,
                alignment: None,
                source: SourceSpan::new(cell_start, cell_start + 1),
            }],
            source: SourceSpan::new(start, start + 1),
        }
    };
    let table: N = Node::Table {
        alignments: vec![None],
        header: vec![row(10, 11)],
        rows: vec![row(20, 21), row(30, 31)],
        source: SourceSpan::new(0, 40),
    };

    let mut seen = Vec::new();
    table.for_each_child(|child| {
        assert!(
            matches!(child, Node::TableRow { .. }),
            "table's direct children must be rows, got {child:?}"
        );
        seen.push(child.source().start);
    });
    assert_eq!(seen, vec![10, 20, 30]);

    // Reaching a cell takes a second level of traversal.
    let mut cells = Vec::new();
    table.for_each_child(|row| row.for_each_child(|cell| cells.push(cell.source().start)));
    assert_eq!(cells, vec![11, 21, 31]);
}

fn nested_tree() -> N {
    Node::Document {
        children: vec![
            Node::Paragraph {
                children: vec![
                    Node::Text {
                        text: "a".to_owned(),
                        source: SourceSpan::new(2, 3),
                    },
                    Node::Strong {
                        children: vec![Node::Text {
                            text: "b".to_owned(),
                            source: SourceSpan::new(4, 5),
                        }],
                        source: SourceSpan::new(3, 6),
                    },
                ],
                source: SourceSpan::new(1, 7),
            },
            Node::Table {
                alignments: vec![None],
                header: vec![Node::TableRow {
                    cells: Vec::new(),
                    source: SourceSpan::new(8, 9),
                }],
                rows: vec![Node::TableRow {
                    cells: Vec::new(),
                    source: SourceSpan::new(10, 11),
                }],
                source: SourceSpan::new(7, 12),
            },
        ],
        source: SourceSpan::new(0, 13),
    }
}

// [spec:mw:req:core.traversal.walk-preorder/test]
#[test]
fn walk_visits_receiver_first_then_descendants_in_preorder() {
    let tree = nested_tree();
    let mut seen = Vec::new();
    tree.walk(&mut |node| seen.push(node.source().start));

    // Receiver, then pre-order document order, with the table's header
    // row ahead of its body row.
    assert_eq!(seen, vec![0, 1, 2, 3, 4, 7, 8, 10]);
}

// [spec:mw:req:core.traversal.mutable-mirror/test]
#[test]
fn for_each_child_mut_mirrors_for_each_child_variant_for_variant() {
    for (index, node) in all_variants().into_iter().enumerate() {
        let mut shared = Vec::new();
        node.for_each_child(|child| shared.push(child.source().start));

        let mut node = node;
        let mut exclusive = Vec::new();
        node.for_each_child_mut(|child| exclusive.push(child.source().start));

        assert_eq!(shared, exclusive, "mirror mismatch for variant {index}");
    }
}

// [spec:mw:req:core.traversal.mutable-mirror/test]
#[test]
fn walk_mut_mirrors_walk_and_can_rewrite_in_place() {
    let tree = nested_tree();
    let mut shared = Vec::new();
    tree.walk(&mut |node| shared.push(node.source().start));

    let mut mutable = nested_tree();
    let mut exclusive = Vec::new();
    mutable.walk_mut(&mut |node| {
        exclusive.push(node.source().start);
        node.source_mut().end += 100;
    });
    assert_eq!(shared, exclusive);

    // The callback's writes landed on the tree, not on copies.
    let mut ends = Vec::new();
    mutable.walk(&mut |node| ends.push(node.source().end));
    assert_eq!(ends, vec![113, 107, 103, 106, 105, 112, 109, 111]);
}

// [spec:mw:req:core.traversal.mutable-mirror/test]
#[test]
fn walk_mut_applies_callback_before_descending_so_clearing_prunes() {
    let mut tree = nested_tree();
    let mut seen = Vec::new();
    tree.walk_mut(&mut |node| {
        seen.push(node.source().start);
        // Clearing the Paragraph's children before descent must remove
        // its subtree (starts 2, 3, 4) from the remaining walk.
        if node.source().start == 1
            && let Node::Paragraph { children, .. } = node
        {
            children.clear();
        }
    });

    assert_eq!(seen, vec![0, 1, 7, 8, 10]);
}

// [spec:mw:req:core.span.non-inverted/test]
#[test]
fn parser_constructs_only_non_inverted_spans() {
    // Every construct the parser can build a span for, including the
    // table paths that compute cell and row spans arithmetically.
    let source = "# Heading\n\n\
         Some *emphasis*, **strong**, ~~struck~~, `code`, ==mark==, H~2~O, x^2^.\n\n\
         > [!NOTE] Titled\n> quoted body\n\n\
         - item one\n- [x] done\n\n\
         1. first\n2. second\n\n\
         | a | bb | ccc |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n\n\
         ```rust\nfn main() {}\n```\n\n\
         Term\n: Definition\n\n\
         A footnote[^1] and a [link](/dest \"t\") and ![img](/i).\n\n\
         [^1]: note body\n\n\
         ---\n\n\
         Inline $a+b$ and display $$c$$.\n\n\
         <div>raw</div>\n";

    let mut parser = crate::parser::Parser::<()>::builder().with_all().build();
    let blocks = parser.parse_full_at(source, 0).expect("parse");
    assert!(!blocks.is_empty());

    let mut checked = 0usize;
    for block in &blocks {
        block.node.walk(&mut |node| {
            let span = node.source();
            assert!(
                span.end >= span.start,
                "inverted span {span:?} on {node:?} — len() would underflow"
            );
            assert_eq!(span.len(), span.end - span.start);
            checked += 1;
        });
    }
    // Guard against the assertions silently covering an empty tree.
    assert!(checked > 40, "only {checked} nodes walked");
}

// [spec:mw:req:core.span.non-inverted/test]
#[test]
fn non_inverted_span_len_is_the_difference_and_empty_span_has_zero_len() {
    assert_eq!(SourceSpan::new(3, 7).len(), 4);
    assert_eq!(SourceSpan::new(7, 7).len(), 0);
    assert!(SourceSpan::new(7, 7).is_empty());
    assert!(!SourceSpan::new(7, 8).is_empty());
}
