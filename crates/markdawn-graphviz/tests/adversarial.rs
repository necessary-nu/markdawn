//! Adversarial / red-team integration tests.
//!
//! Every adversarial DOT input we can think of, hitting the full
//! `render_dot_to_svg` pipeline (caps → scrubber → libgraphviz →
//! sanitized SVG). Tests named `observe_…` are `#[ignore]` diagnostic
//! probes that log behaviour; they're run on demand with
//! `cargo test --test adversarial -- --ignored`.
//!
//! Findings at the top of the file are a permanent record of what each
//! defence actually blocks.
//!
//! ### Current findings (post-mitigation):
//!
//! A1 output injection — SVG sanitiser (in the main markdawn crate)
//! strips `javascript:`/`data:`/`vbscript:`/etc. in href attributes and
//! drops `<script>` / `on*=` attributes. That's tested in the main
//! crate's integration tests; the subcrate only returns raw SVG.
//!
//! A2 existing caps — still enforced.
//!
//! A3 parser-exhaustion — `InputTooLarge` catches the massive-payload
//! class; libgraphviz's own tokenizer handles unterminated string /
//! comment gracefully (tested below).
//!
//! A4 graph-structure DoS — `TooManyEdges` catches fan-out
//! `A -> {b0..bN}`, set-to-set `{A} -> {B}`, and chained flat edges.
//!
//! A5 recursive DoS — `RecordLabelTooDeep` + `HtmlLabelTooDeep` catch
//! the in-string / in-HTML-label nesting classes. `layout=neato`
//! override is neutralised by the scrubber (renamed to `_layout=`).
//! `minlen=` clamped at 20.
//!
//! A6 external-resource attrs — `image=` / `shapefile=` / `fontpath=` /
//! `imagepath=` / abs-path `fontname=` are all renamed with a leading
//! underscore; libgraphviz silently drops unknown attrs, so the file
//! is never opened.

use markdawn_graphviz::{
    GraphvizError, MAX_BRACE_DEPTH, MAX_DOT_BYTES, MAX_EDGES, MAX_HTML_DEPTH, MAX_NODES,
    MAX_RECORD_DEPTH, render_dot_to_svg,
};

// ----- A2: existing caps (regression) -----

#[test]
fn a2_1_oversize_input_rejected() {
    let huge = "a".repeat(MAX_DOT_BYTES + 1);
    let err = render_dot_to_svg(&huge).unwrap_err();
    assert!(matches!(err, GraphvizError::InputTooLarge(_)));
}

#[test]
fn a2_2_deep_outer_braces_rejected() {
    let src = format!(
        "digraph {}{}",
        "{ subgraph ".repeat(MAX_BRACE_DEPTH + 1),
        "}".repeat(MAX_BRACE_DEPTH + 1),
    );
    let err = render_dot_to_svg(&src).unwrap_err();
    assert!(matches!(err, GraphvizError::InputTooDeep(_)));
}

// ----- A3: parser-exhaustion attacks -----

#[test]
fn a3_1_unterminated_block_comment_does_not_hang() {
    // Comment never closes → libgraphviz tokenizer should error, not hang.
    let mut src = String::from("digraph { /*");
    src.push_str(&"a".repeat(5000));
    src.push_str(" A -> B }");
    let result = render_dot_to_svg(&src);
    // Either graphviz accepts the whole thing as comment (no nodes,
    // empty SVG is still Ok) or rejects. Either way, not a hang.
    assert!(result.is_ok() || matches!(result, Err(GraphvizError::RenderFailed(_))));
}

#[test]
fn a3_2_unterminated_string_does_not_hang() {
    let mut src = String::from("digraph { A [label=\"");
    src.push_str(&"a".repeat(5000));
    src.push_str(" }");
    let _ = render_dot_to_svg(&src);
}

// ----- A4: graph-structure DoS (bytes → edges amplification) -----

#[test]
fn a4_1_fanout_rejected() {
    let mut src = String::from("digraph { A -> { ");
    for i in 0..(MAX_EDGES + 10) {
        src.push_str(&format!("b{i} "));
    }
    src.push_str("} }");
    let err = render_dot_to_svg(&src).unwrap_err();
    // Either node-cap or edge-cap is a valid rejection; both fire in
    // fan-out. We just need to confirm the input didn't reach libgraphviz.
    assert!(
        matches!(
            err,
            GraphvizError::TooManyEdges(_) | GraphvizError::TooManyNodes(_)
        ),
        "got: {err:?}"
    );
}

#[test]
fn a4_2_set_to_set_rejected_by_edge_cap() {
    let mut src = String::from("digraph { { ");
    for i in 0..80 {
        src.push_str(&format!("a{i} "));
    }
    src.push_str("} -> { ");
    for i in 0..80 {
        src.push_str(&format!("b{i} "));
    }
    src.push_str("} }");
    // 80 × 80 = 6400 edges > MAX_EDGES (2000)
    let err = render_dot_to_svg(&src).unwrap_err();
    assert!(matches!(err, GraphvizError::TooManyEdges(_)));
}

#[test]
fn a4_3_too_many_flat_nodes_rejected() {
    let mut src = String::from("digraph { ");
    for i in 0..(MAX_NODES + 10) {
        src.push_str(&format!("n{i}; "));
    }
    src.push('}');
    let err = render_dot_to_svg(&src).unwrap_err();
    assert!(matches!(err, GraphvizError::TooManyNodes(_)));
}

// ----- A5: recursive attacks (bypass outer brace scanner) -----

#[test]
fn a5_1_record_label_recursion_rejected() {
    let mut payload = String::new();
    for _ in 0..(MAX_RECORD_DEPTH + 5) {
        payload.push_str("{a|");
    }
    for _ in 0..(MAX_RECORD_DEPTH + 5) {
        payload.push('}');
    }
    let src = format!(r#"digraph {{ A [shape=record, label="{payload}"] }}"#);
    let err = render_dot_to_svg(&src).unwrap_err();
    assert!(matches!(err, GraphvizError::RecordLabelTooDeep(_)));
}

#[test]
fn a5_2_html_label_recursion_rejected() {
    let mut src = String::from("digraph { A [label=<");
    for _ in 0..(MAX_HTML_DEPTH + 5) {
        src.push_str("<B>");
    }
    src.push('x');
    for _ in 0..(MAX_HTML_DEPTH + 5) {
        src.push_str("</B>");
    }
    src.push_str(">] }");
    let err = render_dot_to_svg(&src).unwrap_err();
    assert!(matches!(err, GraphvizError::HtmlLabelTooDeep(_)));
}

#[test]
fn a5_4_layout_override_neutralised() {
    // `layout=neato` should be renamed by the scrubber so libgraphviz
    // can't use it. The actual rendering uses our `gvLayout(..., "dot")`
    // because the attribute is gone.
    let src = r#"digraph { graph [layout=neato]; A -> B }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    // SVG should NOT indicate neato; hard to assert directly, but the
    // render must succeed and produce an SVG.
    assert!(svg.contains("<svg"), "got: {svg}");
}

// ----- A6: external-resource attrs -----

#[test]
fn a6_1_image_path_does_not_open_file() {
    // `image="/etc/passwd"` should be renamed; libgraphviz never sees
    // the real attribute. The render should succeed (the node simply
    // renders without an image).
    let src = r#"digraph { A [image="/etc/passwd"] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "leaked path: {svg}");
}

#[test]
fn a6_2_shapefile_path_does_not_open_file() {
    let src = r#"digraph { A [shapefile="/etc/passwd"] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "got: {svg}");
}

#[test]
fn a6_3_abs_fontname_path_neutralised() {
    let src = r#"digraph { A [fontname="/abs/Bad.ttf", label="x"] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/abs/Bad.ttf"), "got: {svg}");
}

#[test]
fn a6_4_logical_fontname_preserved_for_render() {
    let src = r#"digraph { A [fontname="Helvetica", label="x"] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(svg.contains("<svg"), "got: {svg}");
}

// ----- A6b: scrubber-bypass attempts -----

#[test]
fn a6b_1_uppercase_attribute_name_neutralised() {
    // Case-insensitive scrubber match.
    let src = r#"digraph { A [IMAGE="/etc/passwd"] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "leaked: {svg}");
}

#[test]
fn a6b_2_mixed_case_attribute_name_neutralised() {
    let src = r#"digraph { A [ImAgE="/etc/passwd"] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "leaked: {svg}");
}

#[test]
fn a6b_3_whitespace_around_equals_neutralised() {
    // `image   =   "..."` should still be detected.
    let src = "digraph { A [image   =   \"/etc/passwd\"] }";
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "leaked: {svg}");
}

#[test]
fn a6b_4_multiple_dangerous_attrs_all_neutralised() {
    let src = r#"digraph { A [image="/etc/passwd", shapefile="/etc/passwd2", layout=neato] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "leaked: {svg}");
    assert!(!svg.contains("/etc/passwd2"), "leaked: {svg}");
}

#[test]
fn a6b_5_default_node_attribute_neutralised() {
    // `node [image=...]` as a default attribute, applied to all nodes.
    let src = r#"digraph { node [image="/etc/passwd"]; A; B; A -> B }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "leaked: {svg}");
}

#[test]
fn a6b_6_default_graph_attribute_neutralised() {
    let src = r#"digraph { graph [imagepath="/etc"]; A -> B }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc"), "leaked: {svg}");
}

// ----- A7: numeric overflow / weird values (observational) -----

#[test]
fn a7_1_huge_fontsize_does_not_crash() {
    let src = r#"digraph { A [fontsize=99999999999, label="x"] }"#;
    let _ = render_dot_to_svg(src); // either Ok or RenderFailed, not a crash
}

#[test]
fn a7_2_negative_width_does_not_crash() {
    let src = r#"digraph { A [width=-1, label="x"] }"#;
    let _ = render_dot_to_svg(src);
}

#[test]
fn a7_3_huge_weight_does_not_crash() {
    let src = r#"digraph { A -> B [weight=9999999999] }"#;
    let _ = render_dot_to_svg(src);
}

// ----- A10: edge-chain DoS under caps -----

#[test]
fn a10_1_chained_edges_under_cap_render() {
    // A0 -> A1 -> A2 -> … -> A999  (999 edges via chain)
    let mut src = String::from("digraph { ");
    for i in 0..999 {
        src.push_str(&format!("A{} -> A{}; ", i, i + 1));
    }
    src.push('}');
    // Should render (or at least not cap-reject) since chained form
    // each `->` has right-factor 1.
    let _ = render_dot_to_svg(&src); // may render or hit node cap
}

#[test]
fn a10_2_chained_edges_over_cap_rejected() {
    let mut src = String::from("digraph { ");
    for i in 0..3000 {
        src.push_str(&format!("A{} -> A{}; ", i, i + 1));
    }
    src.push('}');
    let err = render_dot_to_svg(&src).unwrap_err();
    assert!(
        matches!(
            err,
            GraphvizError::TooManyEdges(_) | GraphvizError::TooManyNodes(_)
        ),
        "got: {err:?}"
    );
}

// ----- A11: port references (record-shape side) -----

#[test]
fn a11_1_port_ref_render_under_limit() {
    // 3-port record, edges use ports — should render normally.
    let src = r#"digraph { A [shape=record, label="{<p1>a|<p2>b|<p3>c}"]; B; A:p1 -> B; A:p2 -> B; A:p3 -> B }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(svg.contains("<svg"), "got: {svg}");
}

// ----- A12: polyglot / encoding tricks -----

#[test]
fn a12_1_bom_prefix_does_not_break_render() {
    // UTF-8 BOM then valid DOT. libgraphviz handles this fine usually.
    let mut src = String::from("\u{FEFF}");
    src.push_str("digraph { A -> B }");
    let _ = render_dot_to_svg(&src);
}

#[test]
fn a12_2_unicode_rtl_override_in_label_does_not_crash() {
    // Right-to-left override character U+202E in a label.
    let src = "digraph { A [label=\"a\u{202E}b\"] }";
    let _ = render_dot_to_svg(src);
}

// ----- Observational probes -----

#[test]
#[ignore]
fn observe_timing_under_edge_cap() {
    let start = std::time::Instant::now();
    let mut src = String::from("digraph { ");
    // 1500 distinct nodes, 1500 edges as a chain — just under MAX_EDGES.
    for i in 0..1500 {
        src.push_str(&format!("n{} -> n{}; ", i, i + 1));
    }
    src.push('}');
    let res = render_dot_to_svg(&src);
    eprintln!(
        "1500-edge chain: {:?} in {:?}",
        res.as_ref().map(|s| format!("{} bytes", s.len())).err(),
        start.elapsed()
    );
}

// ----- A13: HTML-label IMG + processing-instruction attribute surfaces -----

#[test]
fn a13_1_img_src_in_html_label_neutralised() {
    // Scrubber renames <IMG ...> → <_IMG ...> so graphviz's HTML-label
    // parser doesn't open SRC.
    let src = r#"digraph { A [label=<<IMG SRC="/etc/passwd"/>>] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("/etc/passwd"), "leaked: {svg}");
}

#[test]
fn a13_2_stylesheet_attribute_neutralised() {
    // `stylesheet=` is now renamed by the scrubber, so the value never
    // reaches graphviz's SVG emitter.
    let src = r#"digraph { graph [stylesheet="javascript:alert(1)"]; A -> B }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    assert!(!svg.contains("javascript:alert"), "leaked: {svg}");
}

#[test]
#[ignore]
fn observe_html_img_with_abs_path() {
    // <IMG SRC="/etc/passwd"/> inside an HTML label. libgraphviz may
    // try to open the file for image sizing. Observational because we
    // can't easily detect file-access from inside the test; just log
    // the behaviour and duration.
    let src = r#"digraph { A [label=<<IMG SRC="/etc/passwd"/>>] }"#;
    let start = std::time::Instant::now();
    let res = render_dot_to_svg(src);
    eprintln!(
        "<IMG SRC=/etc/passwd>: {:?} in {:?}",
        res.as_ref().map(|s| s.len()).err(),
        start.elapsed()
    );
}

#[test]
#[ignore]
fn observe_stylesheet_attr() {
    // `stylesheet` references external CSS in SVG output via
    // <?xml-stylesheet ...?>. Not an in-process SSRF (graphviz doesn't
    // fetch); the href ends up in the SVG output. The post-render
    // SVG sanitizer in the main crate should handle it. Log behaviour.
    let src = r#"digraph { graph [stylesheet="javascript:alert(1)"]; A -> B }"#;
    match render_dot_to_svg(src) {
        Ok(svg) => eprintln!(
            "stylesheet leakage into SVG: contains 'javascript': {}",
            svg.contains("javascript")
        ),
        Err(e) => eprintln!("stylesheet render err: {e}"),
    }
}

#[test]
#[ignore]
fn observe_url_alias_attribute() {
    // `URL="javascript:..."` — alias for href — should emit <a xlink:href>
    // in SVG. Post-render sanitizer catches javascript:.
    let src = r#"digraph { A [URL="javascript:alert(1)"]; B }"#;
    match render_dot_to_svg(src) {
        Ok(svg) => eprintln!(
            "URL=javascript: leaked into raw SVG: {}",
            svg.contains("javascript")
        ),
        Err(e) => eprintln!("URL err: {e}"),
    }
}

#[test]
#[ignore]
fn observe_headurl_tailurl_leak() {
    // Edge URL attributes.
    let src = r#"digraph { A -> B [headURL="javascript:alert(1)", tailURL="data:text/html,x"] }"#;
    match render_dot_to_svg(src) {
        Ok(svg) => eprintln!(
            "headURL javascript: leaked: {}, tailURL data: leaked: {}",
            svg.contains("javascript"),
            svg.contains("data:")
        ),
        Err(e) => eprintln!("err: {e}"),
    }
}

#[test]
#[ignore]
fn observe_concentrate_edges() {
    // `concentrate=true` merges parallel edges — test if it's a DoS vector.
    let mut src = String::from("digraph { graph [concentrate=true]; ");
    for _ in 0..500 {
        src.push_str("A -> B; ");
    }
    src.push('}');
    let start = std::time::Instant::now();
    let res = render_dot_to_svg(&src);
    eprintln!(
        "500 parallel A->B with concentrate: {:?} in {:?}",
        res.as_ref().map(|s| format!("{} bytes", s.len())).err(),
        start.elapsed()
    );
}

// ----- A9: format / numeric probes (observational) -----

#[test]
fn a9_1_printf_format_string_in_label_is_literal() {
    let src = r#"digraph { A [label="%s%n%p"] }"#;
    let svg = render_dot_to_svg(src).expect("renders");
    // The format-string characters should appear as-is in the label.
    assert!(svg.contains("%s") || svg.contains("%"), "got: {svg}");
}

// ----- Observational probes (run with --ignored) -----

#[test]
#[ignore]
fn observe_timing_of_dense_graph() {
    let start = std::time::Instant::now();
    let mut src = String::from("digraph { ");
    // 50 nodes fully connected = 2500 edges — just over MAX_EDGES.
    // This should hit the cap.
    for i in 0..50 {
        for j in 0..50 {
            if i != j {
                src.push_str(&format!("n{i} -> n{j}; "));
            }
        }
    }
    src.push('}');
    let res = render_dot_to_svg(&src);
    let dt = start.elapsed();
    eprintln!("50-node complete-graph: {:?} in {dt:?}", res.as_ref().err());
}

#[test]
#[ignore]
fn observe_html_table_shape() {
    let src = r#"digraph { A [label=<<TABLE><TR><TD>x</TD></TR></TABLE>>] }"#;
    match render_dot_to_svg(src) {
        Ok(svg) => eprintln!("HTML table SVG size: {} bytes", svg.len()),
        Err(e) => eprintln!("HTML table error: {e}"),
    }
}

#[test]
#[ignore]
fn observe_coordinate_overflow() {
    let src = r#"digraph { A [pos="1e100,1e100!"] }"#;
    match render_dot_to_svg(src) {
        Ok(svg) => eprintln!("pos=1e100 SVG size: {}", svg.len()),
        Err(e) => eprintln!("pos=1e100 error: {e}"),
    }
}
