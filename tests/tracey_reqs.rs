#![cfg(feature = "tracey")]

use markdawn::tracey::{RenderOptions, parse_rule_id, render};

#[test]
fn extracts_paragraph_req() {
    let md = "r[my.req] This MUST be foo.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "my.req");
    assert_eq!(doc.reqs[0].id.version, 0);
    assert_eq!(doc.reqs[0].line, 1);
    assert_eq!(doc.reqs[0].marker_span.offset, 0);
    assert_eq!(doc.reqs[0].marker_span.length, 9); // "r[my.req]"
}

#[test]
fn extracts_versioned_req() {
    let md = "r[my.req+2] body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "my.req");
    assert_eq!(doc.reqs[0].id.version, 2);
}

#[test]
fn rejects_plus_zero_via_parse() {
    // `+0` is rejected at parse time; parse_rule_id returns None.
    assert!(parse_rule_id("my.req+0").is_none());
}

#[test]
fn extracts_blockquote_req() {
    let md = "> r[my.req] Body inside a blockquote.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "my.req");
    // marker_span points at the marker, NOT the leading `> `.
    let s = doc.reqs[0].marker_span;
    assert_eq!(&md[s.offset..s.offset + s.length], "r[my.req]");
}

#[test]
fn extracts_three_paragraph_reqs_in_order() {
    let md = concat!(
        "r[req.one] First.\n",
        "\n",
        "r[req.two] Second.\n",
        "\n",
        "r[req.three] Third.\n",
    );
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 3);
    let ids: Vec<&str> = doc.reqs.iter().map(|r| r.id.base.as_str()).collect();
    assert_eq!(ids, vec!["req.one", "req.two", "req.three"]);
}

#[test]
fn extracts_headings() {
    let md = "# Top\n\n## Subsection\n\nBody text.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.headings.len(), 2);
    assert_eq!(doc.headings[0].title, "Top");
    assert_eq!(doc.headings[0].level, 1);
    assert_eq!(doc.headings[0].id, "top");
    assert_eq!(doc.headings[1].title, "Subsection");
    assert_eq!(doc.headings[1].level, 2);
}

#[test]
fn unmarked_paragraph_not_a_req() {
    let md = "Just a paragraph.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert!(doc.reqs.is_empty());
}

#[test]
fn frontmatter_yaml_excluded_from_offsets() {
    let md = "---\ntitle: spec\nweight: 5\n---\nr[my.req] body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert!(doc.frontmatter.is_some());
    let fm = doc.frontmatter.unwrap();
    assert_eq!(fm.title, "spec");
    assert_eq!(fm.weight, 5);
    assert_eq!(doc.reqs.len(), 1);
    // marker_offset should point at `r[` in the original source, after the
    // frontmatter region, not at offset 0.
    let s = doc.reqs[0].marker_span;
    assert_eq!(&md[s.offset..s.offset + s.length], "r[my.req]");
}

#[test]
fn req_html_contains_default_handler_anchor() {
    let md = "r[my.req] Body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    // DefaultReqHandler renders <div class="req" id="r--my.req">
    assert!(doc.html.contains("class=\"req\""));
    assert!(doc.html.contains("id=\"r--my.req\""));
}

#[test]
fn inline_code_handler_can_replace_code_spans() {
    use markdawn::tracey::InlineCodeHandler;

    struct Replacer;
    impl InlineCodeHandler for Replacer {
        fn render(&self, code: &str) -> Option<String> {
            if code == "r[foo]" {
                Some("<a class=\"rule-ref\">r[foo]</a>".to_string())
            } else {
                None
            }
        }
    }

    let md = "See `r[foo]` and `other`.\n";
    let opts = RenderOptions::default().with_inline_code_handler(Replacer);
    let doc = render(md, &opts).unwrap();
    assert!(doc.html.contains("class=\"rule-ref\""));
    // The non-matching code span uses default `<code>` rendering.
    assert!(doc.html.contains("<code>other</code>"));
}

#[test]
fn inline_code_spans_collected() {
    let md = "Use `foo` and `bar` here.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    let contents: Vec<&str> = doc
        .inline_code_spans
        .iter()
        .map(|s| s.content.as_str())
        .collect();
    assert_eq!(contents, vec!["foo", "bar"]);
}

#[test]
fn extracts_constraint_marker() {
    use markdawn::tracey::ReqKind;
    let md = "r![determinism]\nThe system MUST be deterministic.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "determinism");
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
    let s = doc.reqs[0].marker_span;
    assert_eq!(&md[s.offset..s.offset + s.length], "r![determinism]");
}

#[test]
fn extracts_constraint_in_blockquote() {
    use markdawn::tracey::ReqKind;
    let md = "> r![durability]\n> Once a write returns success its effect MUST survive crashes.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "durability");
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
    let s = doc.reqs[0].marker_span;
    assert_eq!(&md[s.offset..s.offset + s.length], "r![durability]");
}

#[test]
fn ordinary_and_constraint_distinguished() {
    use markdawn::tracey::ReqKind;
    let md = concat!(
        "r![determinism]\n",
        "Cross-cutting property.\n",
        "\n",
        "r[storage.put]\n",
        "Ordinary req.\n",
    );
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 2);
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
    assert_eq!(doc.reqs[1].kind, ReqKind::Definition);
}

#[test]
fn constraint_accepts_single_segment_id() {
    use markdawn::tracey::ReqKind;
    let md = "r![foo]\nSingle-segment id is fine for a constraint.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "foo");
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
}

#[test]
fn constraint_with_version_suffix() {
    use markdawn::tracey::ReqKind;
    let md = "r![determinism+2]\nBumped constraint.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "determinism");
    assert_eq!(doc.reqs[0].id.version, 2);
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
}
