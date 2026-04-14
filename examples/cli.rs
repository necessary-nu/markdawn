use std::io::Read;

use markdawn::{HtmlAdapter, Parser, StreamItem, VecSink};

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();

    let mut parser = Parser::<(), ()>::new();
    let mut sink = VecSink::new();
    parser.push_str(&input, &mut sink).unwrap();
    parser.finish(&mut sink).unwrap();

    let adapter = HtmlAdapter::new();
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
