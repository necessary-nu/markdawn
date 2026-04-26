#![cfg(feature = "typst")]

use markdawn::{HtmlAdapter, Node, Parser, StreamItem, VecSink};

fn parse_final_nodes(input: &str) -> Vec<Node<()>> {
    let mut parser = Parser::<()>::builder().with_gfm().build();
    let mut sink = VecSink::new();
    parser.push_str(input, &mut sink).unwrap();
    parser.finish(&mut sink).unwrap();

    let mut order = Vec::new();
    let mut values = Vec::<(u64, Node<()>)>::new();

    for item in sink.items() {
        match item {
            StreamItem::Upsert(emission) => {
                if let Some((_, node)) = values.iter_mut().find(|(id, _)| *id == emission.id) {
                    *node = emission.node.clone();
                } else {
                    order.push(emission.id);
                    values.push((emission.id, emission.node.clone()));
                }
            }
            StreamItem::Delete { id } => {
                order.retain(|existing| existing != id);
                values.retain(|(existing, _)| existing != id);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|id| {
            values
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, n)| n.clone())
        })
        .collect()
}

fn render_html_with_typst(input: &str) -> String {
    let nodes = parse_final_nodes(input);
    let adapter = HtmlAdapter::new().with_typst();
    let mut out = String::new();
    for node in &nodes {
        let item = StreamItem::Upsert(markdawn::Emission {
            id: 0,
            provisional: false,
            node: node.clone(),
        });
        if let StreamItem::Upsert(e) = adapter.render(item).unwrap() {
            out.push_str(&e.node);
        }
    }
    out
}

#[test]
fn typst_fence_emits_inline_svg() {
    let md = "```typst\n\
              #import \"@preview/syntree:0.2.1\": syntree\n\
              #syntree(\"[S [NP cat] [VP sat]]\")\n\
              ```\n";
    let html = render_html_with_typst(md);
    assert!(html.contains("<svg"), "expected inline SVG, got:\n{html}");
    assert!(
        !html.contains("<pre>"),
        "expected svg only, no fallback pre block:\n{html}"
    );
}

#[test]
fn malformed_typst_falls_back_to_pre_code() {
    let md = "```typst\n#let x = (\n```\n";
    let html = render_html_with_typst(md);
    assert!(
        !html.contains("<svg"),
        "malformed source should not produce svg:\n{html}"
    );
    assert!(
        html.contains("<pre>") && html.contains("language-typst"),
        "expected pre/code fallback with language-typst class:\n{html}"
    );
}

#[test]
fn non_typst_fence_is_unaffected() {
    let md = "```text\nplain text\n```\n";
    let html = render_html_with_typst(md);
    assert!(
        html.contains("<pre>") && html.contains("language-text"),
        "text block should round-trip as pre/code:\n{html}"
    );
    assert!(!html.contains("<svg"));
}

#[test]
fn typst_fence_disabled_when_with_typst_not_called() {
    // Default HtmlAdapter (no with_typst) should leave typst fences as code.
    let md = "```typst\n= Heading\n```\n";
    let nodes = parse_final_nodes(md);
    let adapter = HtmlAdapter::new();
    let mut out = String::new();
    for node in &nodes {
        let item = StreamItem::Upsert(markdawn::Emission {
            id: 0,
            provisional: false,
            node: node.clone(),
        });
        if let StreamItem::Upsert(e) = adapter.render(item).unwrap() {
            out.push_str(&e.node);
        }
    }
    assert!(out.contains("<pre>") && out.contains("language-typst"));
    assert!(!out.contains("<svg"));
}
