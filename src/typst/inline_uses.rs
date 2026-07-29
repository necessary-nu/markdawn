//! Inline typst-svg's `<symbol>` + `<use>` glyph references into plain
//! `<g>` + `<path>` so the result survives the W3C-safe-baseline SVG
//! sanitiser (which strips `<use>` wholesale).
//!
//! typst-svg writes each shaped glyph once into a `<defs><symbol
//! id="g…"><path d="…"/></symbol></defs>` block, then references it on
//! each occurrence with `<use xlink:href="#g…" x="X" y="Y" fill="F"
//! fill-rule="…"/>`. Without this pre-pass, sanitising drops every
//! `<use>` (per `safe-baseline-configuration.json`), and the diagram
//! ends up text-less.
//!
//! Strategy: single-pass scanner. First sweep collects each
//! `<symbol id="…">…</symbol>` body into a map. Second sweep copies
//! the SVG verbatim, dropping every `<symbol>` block and replacing
//! every `<use … />` with `<g transform="translate(X Y)" fill="F"
//! fill-rule="…">…body…</g>`. Empty `<defs>` is harmless and survives
//! sanitisation.
//!
//! String-scanning is sufficient because typst-svg's emitter is a
//! deterministic XML serialiser — attribute order is fixed, no
//! awkward whitespace, no nested `<use>` inside `<symbol>`.

use std::collections::HashMap;

// [spec:mw:req:typst.svg.inline-uses]
pub(crate) fn inline_uses(svg: &str) -> String {
    let bodies = collect_symbol_bodies(svg);
    rewrite(svg, &bodies)
}

fn collect_symbol_bodies(svg: &str) -> HashMap<String, String> {
    let mut bodies = HashMap::new();
    let mut cursor = 0usize;
    while let Some(rel) = svg[cursor..].find("<symbol") {
        let open_start = cursor + rel;
        let Some(head_end) = svg[open_start..].find('>').map(|i| open_start + i + 1) else {
            break;
        };
        let head = &svg[open_start..head_end];
        let Some(id) = extract_attr(head, "id") else {
            cursor = head_end;
            continue;
        };
        let Some(close_rel) = svg[head_end..].find("</symbol>") else {
            break;
        };
        let body_end = head_end + close_rel;
        let body = svg[head_end..body_end].to_owned();
        bodies.insert(id.to_owned(), body);
        cursor = body_end + "</symbol>".len();
    }
    bodies
}

fn rewrite(svg: &str, bodies: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut cursor = 0usize;
    while cursor < svg.len() {
        let next_symbol = svg[cursor..].find("<symbol").map(|i| cursor + i);
        let next_use = find_use_tag(svg, cursor);
        match (next_symbol, next_use) {
            (None, None) => {
                out.push_str(&svg[cursor..]);
                break;
            }
            (Some(s), None) => {
                out.push_str(&svg[cursor..s]);
                cursor = skip_symbol_block(svg, s);
            }
            (None, Some(u)) => {
                out.push_str(&svg[cursor..u]);
                cursor = emit_inlined_use(svg, u, bodies, &mut out);
            }
            (Some(s), Some(u)) if s < u => {
                out.push_str(&svg[cursor..s]);
                cursor = skip_symbol_block(svg, s);
            }
            (Some(_), Some(u)) => {
                out.push_str(&svg[cursor..u]);
                cursor = emit_inlined_use(svg, u, bodies, &mut out);
            }
        }
    }
    out
}

fn skip_symbol_block(svg: &str, start: usize) -> usize {
    match svg[start..].find("</symbol>") {
        Some(rel) => start + rel + "</symbol>".len(),
        None => svg.len(),
    }
}

/// Emit one inlined `<use>` and return the cursor position past the
/// original `<use … />` tag.
fn emit_inlined_use(
    svg: &str,
    start: usize,
    bodies: &HashMap<String, String>,
    out: &mut String,
) -> usize {
    let Some(end) = svg[start..].find('>').map(|i| start + i + 1) else {
        out.push_str(&svg[start..]);
        return svg.len();
    };
    let head = &svg[start..end];
    let href = extract_attr(head, "xlink:href")
        .or_else(|| extract_attr(head, "href"))
        .unwrap_or("");
    let id = href.strip_prefix('#').unwrap_or(href);
    let body = bodies.get(id).map(String::as_str).unwrap_or("");
    let x = extract_attr(head, "x").unwrap_or("0");
    let y = extract_attr(head, "y").unwrap_or("0");
    let fill = extract_attr(head, "fill");
    let fill_rule = extract_attr(head, "fill-rule");

    out.push_str("<g transform=\"translate(");
    out.push_str(x);
    out.push(' ');
    out.push_str(y);
    out.push_str(")\"");
    if let Some(f) = fill {
        out.push_str(" fill=\"");
        out.push_str(f);
        out.push('"');
    }
    if let Some(fr) = fill_rule {
        out.push_str(" fill-rule=\"");
        out.push_str(fr);
        out.push('"');
    }
    out.push('>');
    out.push_str(body);
    out.push_str("</g>");
    end
}

/// Find the next `<use ...>` tag at or after `from`, skipping false
/// matches like `<user>` or `<used-name>`.
fn find_use_tag(svg: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some(rel) = svg[cursor..].find("<use") {
        let abs = cursor + rel;
        let next = svg.as_bytes().get(abs + 4).copied();
        if matches!(
            next,
            Some(b' ') | Some(b'/') | Some(b'>') | Some(b'\t') | Some(b'\n')
        ) {
            return Some(abs);
        }
        cursor = abs + 4;
    }
    None
}

fn extract_attr<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let needle = format!(" {name}={quote}");
        if let Some(start) = head.find(&needle) {
            let value_start = start + needle.len();
            if let Some(rel) = head[value_start..].find(quote) {
                return Some(&head[value_start..value_start + rel]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:req:typst.svg.inline-uses/test]
    #[test]
    fn inlines_use_into_g_with_symbol_body() {
        let svg = r##"<svg><defs><symbol id="g1" overflow="visible"><path d="M0 0h10"/></symbol></defs><use xlink:href="#g1" x="5" y="7" fill="#000" fill-rule="nonzero"/></svg>"##;
        let out = inline_uses(svg);
        assert!(!out.contains("<symbol"), "symbols not stripped: {out}");
        assert!(!out.contains("<use"), "use not replaced: {out}");
        assert!(
            out.contains("<g transform=\"translate(5 7)\""),
            "got: {out}"
        );
        assert!(out.contains("fill=\"#000\""), "got: {out}");
        assert!(out.contains("fill-rule=\"nonzero\""), "got: {out}");
        assert!(out.contains(r##"<path d="M0 0h10"/>"##), "got: {out}");
    }

    // [spec:mw:req:typst.svg.inline-uses/test]
    #[test]
    fn unknown_use_id_emits_empty_g() {
        let svg = r##"<svg><use xlink:href="#missing" x="1" y="2"/></svg>"##;
        let out = inline_uses(svg);
        assert!(out.contains("<g transform=\"translate(1 2)\""));
        assert!(out.contains("></g>"));
    }

    #[test]
    fn multiple_uses_resolve_independently() {
        let svg = r##"<svg><symbol id="a"><path d="A"/></symbol><symbol id="b"><path d="B"/></symbol><use xlink:href="#a" x="0" y="0"/><use xlink:href="#b" x="10" y="20"/></svg>"##;
        let out = inline_uses(svg);
        assert!(out.contains(r##"<path d="A"/>"##));
        assert!(out.contains(r##"<path d="B"/>"##));
        assert!(out.contains("translate(0 0)"));
        assert!(out.contains("translate(10 20)"));
    }

    // [spec:mw:req:typst.svg.inline-uses/test]
    #[test]
    fn does_not_eat_unrelated_user_tag() {
        // Hypothetical sibling tag that starts with "use" — must not
        // be treated as a `<use>` by the scanner.
        let svg = "<svg><user-data>x</user-data></svg>";
        let out = inline_uses(svg);
        assert_eq!(out, svg);
    }
}
