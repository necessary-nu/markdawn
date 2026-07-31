use std::io::Read;

use markdawn::{HtmlAdapter, Parser, StreamItem, VecSink};

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();

    let args: Vec<String> = std::env::args().collect();
    let gfm = args.iter().any(|a| a == "--gfm");
    let math = args.iter().any(|a| a == "--math");
    #[cfg(feature = "highlight")]
    let highlight = args.iter().any(|a| a == "--highlight");
    #[cfg(feature = "pikchr")]
    let pikchr = args.iter().any(|a| a == "--pikchr");
    #[cfg(feature = "graphviz")]
    let graphviz = args.iter().any(|a| a == "--graphviz");

    let mut builder = Parser::<()>::builder();
    if gfm {
        builder = builder.with_gfm();
    }
    if math {
        builder = builder.with_math();
    }
    let mut parser = builder.build();
    let mut sink = VecSink::new();
    parser.push_str(&input, &mut sink).unwrap();
    parser.finish(&mut sink).unwrap();

    let adapter = HtmlAdapter::new();
    #[cfg(feature = "highlight")]
    let adapter = if highlight {
        adapter
            .with_highlighter(markdawn::Highlighter::new().expect("compose the language catalog"))
    } else {
        adapter
    };
    #[cfg(feature = "pikchr")]
    let adapter = if pikchr {
        adapter.with_pikchr()
    } else {
        adapter
    };
    #[cfg(feature = "graphviz")]
    let adapter = if graphviz {
        adapter.with_graphviz()
    } else {
        adapter
    };
    let mut order = Vec::new();
    let mut values = Vec::<(u64, String)>::new();

    for item in sink.into_items() {
        match adapter.render(item).unwrap() {
            StreamItem::Upsert(emission) => {
                if let Some((_, html)) = values.iter_mut().find(|(id, _)| *id == emission.id) {
                    *html = emission.node;
                } else {
                    order.push(emission.id);
                    values.push((emission.id, emission.node));
                }
            }
            StreamItem::Delete { id } => {
                order.retain(|existing| *existing != id);
                values.retain(|(existing, _)| *existing != id);
            }
        }
    }

    for id in order {
        if let Some((_, html)) = values.iter().find(|(existing, _)| *existing == id) {
            print!("{}", html);
        }
    }
}
