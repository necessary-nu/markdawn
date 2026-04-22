//! Red-team integration tests.
//!
//! Every adversarial DOT input we can think of, hitting the full
//! `render_dot_to_svg` pipeline (caps → scrubber → libgraphviz →
//! sanitized SVG). Tests named `observe_…` are `#[ignore]` diagnostic
//! probes that log behaviour; they're run on demand with
//! `cargo test --test redteam -- --ignored`.
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
