//! Characterization harness for the HTML adapter. Renders a corpus
//! exercising every node variant through `HtmlAdapter` and prints the
//! concatenated output. Used to verify the vdom-backed refactor of
//! `html.rs` is byte-for-byte identical (capture before, diff after).

use markdawn::{Emission, HtmlAdapter, Node, Parser, StreamItem, VecSink};

fn parse_final_nodes(input: &str) -> Vec<Node<()>> {
    let mut parser = Parser::<()>::builder().with_all().build();
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
        .filter_map(|id| values.iter().find(|(existing, _)| *existing == id))
        .map(|(_, node)| node.clone())
        .collect()
}

fn render(input: &str) -> String {
    render_with(input, HtmlAdapter::new())
}

/// Adapter with every enabled feature's code-block option turned on, to
/// exercise the diagram/highlight render paths.
#[allow(unused_mut)]
fn featured_adapter() -> HtmlAdapter {
    let mut adapter = HtmlAdapter::new();
    #[cfg(feature = "highlight")]
    {
        adapter = adapter.with_highlighter(markdawn::arborium::Highlighter::new());
    }
    #[cfg(feature = "pikchr")]
    {
        adapter = adapter.with_pikchr();
    }
    #[cfg(feature = "graphviz")]
    {
        adapter = adapter.with_graphviz();
    }
    #[cfg(feature = "typst")]
    {
        adapter = adapter.with_typst();
    }
    adapter
}

fn render_featured(input: &str) -> String {
    render_with(input, featured_adapter())
}

fn render_with<R>(input: &str, adapter: HtmlAdapter<R>) -> String
where
    R: markdawn::RenderHtmlCustom<()>,
{
    let nodes = parse_final_nodes(input);
    let mut out = String::new();
    for node in nodes {
        match adapter
            .render(StreamItem::Upsert(Emission {
                id: 0,
                provisional: false,
                node,
            }))
            .unwrap()
        {
            StreamItem::Upsert(emission) => out.push_str(&emission.node),
            StreamItem::Delete { .. } => {}
        }
    }
    out
}

const CORPUS: &[(&str, &str)] = &[
    ("sample.md", include_str!("sample.md")),
    (
        "headings",
        "# h1\n## h2\n### h3\n#### h4\n##### h5\n###### h6\n\nSetext\n===\n\nSub\n---\n",
    ),
    (
        "inline-emphasis",
        "*em* **strong** ***both*** ~~strike~~ `code` H~2~O E=mc^2^ ==mark==\n",
    ),
    (
        "links-images",
        "[plain](http://example.com) [titled](http://example.com \"T\") \
         [js](javascript:alert(1)) [mail](mailto:a@b.com) <http://auto.link>\n\n\
         ![alt](http://example.com/i.png \"title\") ![bad](javascript:alert(1)) \
         ![data](data:image/png;base64,AAAA)\n",
    ),
    (
        "breaks",
        "line one  \nhard break\nsoft break\n\n---\n\n***\n",
    ),
    (
        "blockquote-nested",
        "> outer\n> > inner\n> > > deep\n>\n> back to outer\n",
    ),
    (
        "lists-tight",
        "- a\n- b\n- c\n\n1. one\n2. two\n\n5. five-start\n6. six\n",
    ),
    (
        "lists-loose",
        "- a\n\n- b\n\n- c\n\n1. first\n\n   second para\n\n2. next\n",
    ),
    (
        "lists-nested",
        "- top\n  - mid\n    - deep\n  - mid2\n- top2\n",
    ),
    ("task-items", "- [ ] todo\n- [x] done\n- [ ] another\n"),
    (
        "tables",
        "| left | center | right | default |\n|:-----|:------:|------:|---------|\n| a | b | c | d |\n| e | f | g | h |\n",
    ),
    ("table-header-only", "| h1 | h2 |\n|----|----|\n"),
    (
        "code-blocks",
        "```rust\nfn main() {}\n```\n\n```\nno lang\n```\n\n    indented code\n",
    ),
    (
        "math",
        "Inline $x^2 + y^2$ math.\n\n$$\n\\int_0^1 x\\,dx\n$$\n",
    ),
    (
        "raw-html",
        "<div class=\"x\">block</div>\n\nInline <span>raw</span> html.\n\n<script>alert(1)</script>\n",
    ),
    (
        "alerts",
        "> [!NOTE]\n> note body\n\n> [!TIP] Custom Title\n> tip body\n\n> [!IMPORTANT]\n> imp\n\n> [!WARNING]\n> warn\n\n> [!CAUTION]\n> caution\n",
    ),
    (
        "footnotes",
        "Text with a footnote[^1] and another[^long].\n\n[^1]: First note.\n\n[^long]: A longer\n    multi-line note.\n",
    ),
    (
        "footnote-multiref",
        "First[^a] then again[^a].\n\n[^a]: shared.\n",
    ),
    (
        "definition-lists",
        "Term 1\n: Definition 1\n\nTerm 2\n: Definition 2a\n: Definition 2b\n",
    ),
    (
        "entities-escapes",
        "AT&amp;T &copy; \\* escaped \\` <not a tag> 5 < 6 & 7 > 3\n",
    ),
    ("ansi-fallback", "```ansi\n\x1b[31mred\x1b[0m\n```\n"),
];

const FEATURED_CORPUS: &[(&str, &str)] = &[
    (
        "highlight",
        "```rust\nfn main() {\n    let x = 1;\n}\n```\n\n```python\ndef f(): pass\n```\n",
    ),
    ("pikchr", "```pikchr\nbox \"hello\"\n```\n"),
    ("graphviz", "```dot\ndigraph { a -> b }\n```\n"),
    ("typst", "```typst\n#text[hi]\n```\n"),
];

fn main() {
    let mut out = String::new();
    for (name, input) in CORPUS {
        out.push_str("\n========== ");
        out.push_str(name);
        out.push_str(" ==========\n");
        out.push_str(&render(input));
    }
    for (name, input) in FEATURED_CORPUS {
        out.push_str("\n========== featured:");
        out.push_str(name);
        out.push_str(" ==========\n");
        out.push_str(&render_featured(input));
    }
    print!("{out}");
}
