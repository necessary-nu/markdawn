#![cfg(feature = "tracey")]

use markdawn::tracey::{RenderOptions, ReqKind, parse_rule_id, render};

#[test]
fn extracts_definition_blockquote() {
    let md = "> :[r:my.req]\n> This MUST be foo.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "my.req");
    assert_eq!(doc.reqs[0].id.version, 0);
    assert_eq!(doc.reqs[0].kind, ReqKind::Definition);
    let s = doc.reqs[0].marker_span;
    assert_eq!(&md[s.offset..s.offset + s.length], ":[r:my.req]");
}

#[test]
fn extracts_constraint_blockquote() {
    let md = "> :[!r:determinism]\n> The system MUST be deterministic.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "determinism");
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
    let s = doc.reqs[0].marker_span;
    assert_eq!(&md[s.offset..s.offset + s.length], ":[!r:determinism]");
}

#[test]
fn extracts_versioned_definition() {
    let md = "> :[r:my.req+2]\n> bumped body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "my.req");
    assert_eq!(doc.reqs[0].id.version, 2);
}

#[test]
fn rejects_plus_zero_via_parse() {
    assert!(parse_rule_id("my.req+0").is_none());
}

#[test]
fn rejects_paragraph_form_definition() {
    // Paragraph-form (`r[foo]\nbody.`) is no longer a marker.
    let md = "r[my.req]\nThis is just paragraph text now.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert!(doc.reqs.is_empty(), "paragraph form should NOT extract");
}

#[test]
fn rejects_blockquote_without_leading_colon() {
    // Bare `[r:id]` without the leading `:` is NOT a marker. The leading
    // colon is what distinguishes a tracey marker from prose `[label:value]`
    // brackets and unknown GFM admonition keywords.
    let md = "> [r:my.req]\n> body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert!(
        doc.reqs.is_empty(),
        "form without leading `:` should NOT extract"
    );
}

#[test]
fn rejects_legacy_in_bracket_sigils() {
    // Legacy `[>r:id]`/`[?r:id]` shapes belong to the source-side grammar,
    // not the spec side.
    let md_gt = "> :[>r:foo]\n> body.\n";
    let md_q = "> :[?r:foo]\n> body.\n";
    assert!(
        render(md_gt, &RenderOptions::default())
            .unwrap()
            .reqs
            .is_empty()
    );
    assert!(
        render(md_q, &RenderOptions::default())
            .unwrap()
            .reqs
            .is_empty()
    );
}

#[test]
fn rejects_gfm_alert_blocks() {
    // Standard GFM alerts (`[!NOTE]`, etc.) must not be picked up.
    let md = "> [!NOTE]\n> This is a regular GFM note.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert!(doc.reqs.is_empty(), "GFM alert should NOT extract as a req");
}

#[test]
fn extracts_three_definitions_in_order() {
    let md = concat!(
        "> :[r:req.one]\n> First.\n",
        "\n",
        "> :[r:req.two]\n> Second.\n",
        "\n",
        "> :[r:req.three]\n> Third.\n",
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
fn unmarked_blockquote_not_a_req() {
    let md = "> Just a quoted paragraph.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert!(doc.reqs.is_empty());
}

#[test]
fn frontmatter_yaml_excluded_from_offsets() {
    let md = "---\ntitle: spec\nweight: 5\n---\n> :[r:my.req]\n> body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert!(doc.frontmatter.is_some());
    let fm = doc.frontmatter.as_ref().unwrap();
    assert_eq!(fm.title, "spec");
    assert_eq!(fm.weight, 5);
    assert_eq!(doc.reqs.len(), 1);
    let s = doc.reqs[0].marker_span;
    assert_eq!(&md[s.offset..s.offset + s.length], ":[r:my.req]");
}

#[test]
fn req_html_contains_default_handler_anchor() {
    let md = "> :[r:my.req]\n> Body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    // DefaultReqHandler renders <div class="req" id="r--my.req">
    assert!(doc.html.contains("class=\"req\""));
    assert!(doc.html.contains("id=\"r--my.req\""));
}

#[test]
fn ordinary_and_constraint_distinguished() {
    let md = concat!(
        "> :[!r:determinism]\n> Cross-cutting.\n",
        "\n",
        "> :[r:storage.put]\n> Ordinary.\n",
    );
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 2);
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
    assert_eq!(doc.reqs[1].kind, ReqKind::Definition);
}

#[test]
fn constraint_accepts_single_segment_id() {
    let md = "> :[!r:foo]\n> Single-segment id is fine for a constraint.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "foo");
    assert_eq!(doc.reqs[0].kind, ReqKind::Constraint);
}

#[test]
fn multi_char_prefix() {
    let md = "> :[h2:rfc.7540.streams]\n> body.\n";
    let doc = render(md, &RenderOptions::default()).unwrap();
    assert_eq!(doc.reqs.len(), 1);
    assert_eq!(doc.reqs[0].id.base, "rfc.7540.streams");
}

#[test]
fn inline_code_handler_can_replace_code_spans() {
    use markdawn::tracey::InlineCodeHandler;

    struct Replacer;
    impl InlineCodeHandler for Replacer {
        fn render(&self, code: &str) -> Option<String> {
            if code == "[r:foo]" {
                Some("<a class=\"rule-ref\">r:foo</a>".to_string())
            } else {
                None
            }
        }
    }

    let md = "See `[r:foo]` and `other`.\n";
    let opts = RenderOptions::default().with_inline_code_handler(Replacer);
    let doc = render(md, &opts).unwrap();
    assert!(doc.html.contains("class=\"rule-ref\""));
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
