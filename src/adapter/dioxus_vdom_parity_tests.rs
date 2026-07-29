//! Agreement between the Dioxus adapter and the canonical vdom
//! renderer: both must project the same node tree to the same HTML.

use super::tests::{both, parse_nodes, vdom_html_with};
use super::*;

// [spec:mw:req:dioxus.structure.vdom-agreement+1/test]
#[test]
fn definition_list_wraps_children_in_dl_like_vdom() {
    let options = DioxusOptions {
        definition_lists: true,
        ..Default::default()
    };
    let (dioxus, vdom) = both("term\n: definition\n", options);
    assert!(vdom.contains("<dl>"), "vdom lost its <dl>: {vdom}");
    assert_eq!(dioxus, vdom);
}

// [spec:mw:req:dioxus.structure.vdom-agreement+1/test]
// [spec:mw:req:core.node.table-cell-alignment/test]
#[test]
fn table_cell_inherits_column_alignment_like_vdom() {
    use crate::node::SourceSpan;

    fn cell(text: &str) -> Node<()> {
        Node::TableCell {
            children: vec![Node::Text {
                text: text.to_owned(),
                source: SourceSpan::default(),
            }],
            header: false,
            // No per-cell alignment: it has to come from the
            // table-level `alignments` vector.
            alignment: None,
            source: SourceSpan::default(),
        }
    }
    fn row(cells: Vec<Node<()>>) -> Node<()> {
        Node::TableRow {
            cells,
            source: SourceSpan::default(),
        }
    }

    let table = Node::Table {
        alignments: vec![Some(TableAlignment::Left), Some(TableAlignment::Right)],
        header: vec![row(vec![cell("a"), cell("b")])],
        rows: vec![row(vec![cell("1"), cell("2")])],
        source: SourceSpan::default(),
    };

    let dioxus = dioxus_ssr::render_element(Renderer::default().render_node(&table));
    let vdom = vdom_html_with(
        std::slice::from_ref(&table),
        &crate::adapter::html::HtmlAdapter::new(),
    );

    assert!(vdom.contains(r#"<td align="left">1</td>"#), "vdom: {vdom}");
    assert!(vdom.contains(r#"<td align="right">2</td>"#), "vdom: {vdom}");
    assert!(
        dioxus.contains(r#"<td style="text-align: left">1</td>"#),
        "dioxus: {dioxus}"
    );
    assert!(
        dioxus.contains(r#"<td style="text-align: right">2</td>"#),
        "dioxus: {dioxus}"
    );
}

// [spec:mw:req:dioxus.structure.vdom-agreement+1/test]
#[test]
fn thead_and_tbody_emission_matches_vdom() {
    let options = DioxusOptions {
        gfm: true,
        ..Default::default()
    };
    let (dioxus, vdom) = both("| a | b |\n|---|---|\n", options);
    assert!(vdom.contains("<thead>"), "vdom: {vdom}");
    assert!(!vdom.contains("<tbody>"), "vdom: {vdom}");
    assert!(dioxus.contains("<thead>"), "dioxus: {dioxus}");
    assert!(
        !dioxus.contains("<tbody>"),
        "header-only table needs no tbody: {dioxus}"
    );

    let (dioxus, vdom) = both("| a | b |\n|---|---|\n| 1 | 2 |\n", options);
    assert!(vdom.contains("<tbody>"), "vdom: {vdom}");
    assert!(dioxus.contains("<tbody>"), "dioxus: {dioxus}");
}

// [spec:mw:req:dioxus.structure.vdom-agreement+1/test]
// [spec:adv:req:markdown.url-allowlist/test]
#[test]
fn image_url_screening_matches_vdom() {
    let cases = [
        ("![x](data:image/png;base64,AAAA)", true),
        ("![x](data:image/svg+xml,%3Csvg%2F%3E)", true),
        ("![x](https://example.com/a.png)", true),
        ("![x](data:text/html;base64,AAAA)", false),
        ("![x](javascript:alert(1))", false),
    ];
    for (source, kept) in cases {
        let (dioxus, vdom) = both(source, DioxusOptions::default());
        assert_eq!(
            !vdom.contains(r#"src="""#),
            kept,
            "vdom disagrees with the case table for {source}: {vdom}"
        );
        assert_eq!(
            dioxus.contains(r#"src="""#),
            vdom.contains(r#"src="""#),
            "dioxus and vdom disagree on {source}: {dioxus} vs {vdom}"
        );
    }
}

// [spec:mw:req:dioxus.structure.vdom-agreement+1/test]
#[test]
fn math_hosts_match_vdom() {
    let adapter = crate::adapter::html::HtmlAdapter::new();
    for node in [
        Node::MathBlock {
            math: "x^2".to_owned(),
            source: crate::node::SourceSpan::default(),
        },
        Node::MathInline {
            math: "a + b".to_owned(),
            source: crate::node::SourceSpan::default(),
        },
    ] {
        let dioxus = dioxus_ssr::render_element(Renderer::default().render_node(&node));
        let vdom = vdom_html_with(std::slice::from_ref(&node), &adapter);
        assert_eq!(dioxus, vdom);
    }
}

#[cfg(feature = "terminal")]
// [spec:mw:req:dioxus.html.host-element+1/test]
// [spec:mw:req:dioxus.structure.vdom-agreement+1/test]
#[test]
fn terminal_block_uses_two_pane_wrapper_like_vdom() {
    let (dioxus, vdom) = both("```terminal\nhello\n```", DioxusOptions::default());
    for marker in [
        "terminal-block",
        "terminal-toggle-input",
        "terminal-toggle",
        "terminal-rendered",
        "terminal-source",
        "language-terminal\"",
        "language-terminal-source",
    ] {
        assert!(vdom.contains(marker), "vdom lost {marker}: {vdom}");
        assert!(dioxus.contains(marker), "dioxus lost {marker}: {dioxus}");
    }
}

#[cfg(feature = "typst")]
// [spec:mw:def:dioxus.surface.options+1/test]
// [spec:mw:req:dioxus.html.host-element+1/test]
#[test]
fn typst_block_renders_inline_svg_like_vdom() {
    let source = "```typst\n$x^2$\n```";
    let dioxus = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                typst: true,
                ..Default::default()
            })
            .render(source),
    );
    let vdom = vdom_html_with(
        &parse_nodes(source, DioxusOptions::default()),
        &crate::adapter::html::HtmlAdapter::new().with_typst(),
    );
    assert!(vdom.contains("<svg"), "vdom: {vdom}");
    assert!(dioxus.contains("<svg"), "dioxus: {dioxus}");
    assert!(dioxus.contains(r#"class="typst""#), "dioxus: {dioxus}");
}

#[cfg(feature = "typst")]
// [spec:mw:def:dioxus.surface.options+1/test]
#[test]
fn typst_block_falls_back_when_disabled() {
    let html = dioxus_ssr::render_element(render_markdown(
        "```typst\n$x^2$\n```",
        DioxusOptions::default(),
    ));
    assert!(html.contains(r#"class="language-typst""#), "got: {html}");
    assert!(!html.contains("<svg"), "got: {html}");
}

#[cfg(feature = "typst")]
// [spec:mw:def:dioxus.surface.options+1/test]
#[test]
fn all_options_enable_typst() {
    assert!(DioxusOptions::all().typst);
}
