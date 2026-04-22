//! Tiny end-to-end smoke demo of the ANSI + ```ansi support.
//!
//! Default output is ANSI so running the example in a terminal
//! actually paints colours. Pass `--html` to dump the HTML wrapper
//! markup instead (useful for inspecting the two-pane toggle).
//!
//!   cargo run --features ansi,dioxus,highlight --example ansi_demo
//!   cargo run --features ansi,dioxus,highlight --example ansi_demo -- --html
//!   cargo run --features ansi,dioxus,highlight --example ansi_demo | less -R

use markdawn::{AnsiAdapter, Emission, HtmlAdapter, Node, Parser, StreamItem, VecSink};

const SAMPLE: &str = r#"# Terminal output demo

Markdown runs through markdawn the usual way — **bold**, _italic_,
[links](https://silo.dev).

## Inline ansi

The fence below carries real SGR escapes (check the raw bytes with
`cat -v`):

"#;

fn build_ansi_fence() -> String {
    // ```ansi<CR><LF>…escape codes…<CR><LF>```
    let mut s = String::from("```ansi\n");
    // Cyan "Compiling", green "Finished".
    s.push_str("\x1b[1;36m  Compiling\x1b[0m silo v0.1.0\n");
    s.push_str("\x1b[1;32m    Finished\x1b[0m `release` profile in \x1b[1;33m9.64s\x1b[0m\n");
    s.push_str("\x1b[1;31merror[E0001]\x1b[0m: unknown method `.for-each`\n");
    s.push_str("    \x1b[38;5;246m╭─[\x1b[0m example.si:42:7 \x1b[38;5;246m]\x1b[0m\n");
    s.push_str("\x1b[38;5;246m42 │\x1b[0m   :fn foo\n");
    s.push_str("\x1b[38;5;246m   │\x1b[0m       \x1b[31m╰─ here\x1b[0m\n");
    s.push_str("```\n");
    s
}

fn parse(source: &str) -> Vec<Node<()>> {
    let mut parser: Parser<()> = Parser::builder().with_all().build();
    let mut sink: VecSink<Node<()>> = VecSink::new();
    parser.push_str(source, &mut sink).expect("push_str");
    parser.finish(&mut sink).expect("finish");
    sink.into_items()
        .into_iter()
        .filter_map(|i| match i {
            StreamItem::Upsert(Emission { node, .. }) => Some(node),
            StreamItem::Delete { .. } => None,
        })
        .collect()
}

fn wrap_upsert<N>(node: N, id: u64) -> StreamItem<N> {
    StreamItem::Upsert(Emission {
        id,
        provisional: false,
        node,
    })
}

fn main() {
    let want_html = std::env::args().any(|a| a == "--html");

    let mut source = String::from(SAMPLE);
    source.push_str(&build_ansi_fence());

    let nodes = parse(&source);

    if want_html {
        let adapter = HtmlAdapter::new();
        for (i, node) in nodes.into_iter().enumerate() {
            match adapter.render(wrap_upsert(node, i as u64)).unwrap() {
                StreamItem::Upsert(Emission { node, .. }) => print!("{node}"),
                StreamItem::Delete { .. } => {}
            }
        }
    } else {
        let adapter = AnsiAdapter::new();
        for (i, node) in nodes.into_iter().enumerate() {
            match adapter.render(wrap_upsert(node, i as u64)).unwrap() {
                StreamItem::Upsert(Emission { node, .. }) => print!("{node}"),
                StreamItem::Delete { .. } => {}
            }
        }
    }
}
