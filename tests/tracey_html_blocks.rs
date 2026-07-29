use markdawn::{HtmlAdapter, HtmlFilter, Parser, StreamItem, VecSink};

fn render_html(input: &str) -> String {
    let mut parser = Parser::<()>::builder().with_gfm().with_math().build();
    let mut sink = VecSink::new();
    parser.push_str(input, &mut sink).unwrap();
    parser.finish(&mut sink).unwrap();

    // These cases verify GFM HTML-*block* parsing (where blocks start and
    // end). GFM renders raw HTML verbatim, so observe the parse with the
    // unrestricted filter — the default `safe()` sanitizer would rewrite
    // doctypes/CDATA/PIs and obscure what's being tested.
    let adapter = HtmlAdapter::new().with_html_filter(HtmlFilter::unrestricted());
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

    let mut out = String::new();
    for id in order {
        if let Some((_, html)) = values.iter().find(|(existing, _)| *existing == id) {
            out.push_str(html);
        }
    }
    out
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-3-processing-instruction-start-and-end-conditions/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-5-end-at-matching-end-marker/test]
#[test]
fn gfm_html_block_type_3_processing_instruction_runs_until_question_angle() {
    let html = render_html("<?php\n\n  echo '>';\n\n?>\nokay\n");
    assert_eq!(html, "<?php\n\n  echo '>';\n\n?>\n<p>okay</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-4-declaration-start-and-end-conditions/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-5-end-at-matching-end-marker/test]
#[test]
fn gfm_html_block_type_4_declaration_ends_on_angle_bracket() {
    let html = render_html("<!DOCTYPE html>\nokay\n");
    assert_eq!(html, "<!DOCTYPE html>\n<p>okay</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-5-cdata-start-and-end-conditions/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-5-end-at-matching-end-marker/test]
#[test]
fn gfm_html_block_type_5_cdata_runs_until_double_bracket_close() {
    let html = render_html(
        "<![CDATA[\nfunction matchwo(a,b)\n{\n  if (a < b && a < 0) then {\n    return 1;\n\n  } else {\n\n    return 0;\n  }\n}\n]]>\nokay\n",
    );
    assert_eq!(
        html,
        "<![CDATA[\nfunction matchwo(a,b)\n{\n  if (a < b && a < 0) then {\n    return 1;\n\n  } else {\n\n    return 0;\n  }\n}\n]]>\n<p>okay</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.ends-at-matching-end-condition-or-container-end/test]
#[test]
fn gfm_html_block_ends_at_container_end_when_no_further_end_condition_appears() {
    let html = render_html("> <div>\n> foo\n\nbar\n");
    assert_eq!(
        html,
        "<blockquote>\n<div>\nfoo\n</blockquote>\n<p>bar</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.starts-after-up-to-three-spaces-of-indentation/test]
#[test]
fn gfm_html_block_allows_up_to_three_spaces_of_indentation() {
    let html = render_html("  <!-- foo -->\nokay\n");
    assert_eq!(html, "  <!-- foo -->\n<p>okay</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.inner-html-does-not-change-active-html-block-type/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.inner-start-conditions-do-not-affect-parser-state/test]
#[test]
fn gfm_html_block_inner_start_conditions_do_not_change_type_6_state() {
    let html =
        render_html("<table><tr><td>\n<pre>\n**Hello**,\n\n_world_.\n</pre>\n</td></tr></table>\n");
    assert_eq!(
        html,
        "<table><tr><td>\n<pre>\n**Hello**,\n<p><em>world</em>.\n</pre></p>\n</td></tr></table>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-5-without-end-marker-close-at-container-end/test]
#[test]
fn gfm_html_block_type_1_without_end_marker_closes_at_document_end() {
    let html = render_html("<style\n  type=\"text/css\">\n\nfoo\n");
    assert_eq!(html, "<style\n  type=\"text/css\">\n\nfoo\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.single-line-block-when-start-and-end-both-match/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.end-marker-may-occur-on-same-line-as-start/test]
#[test]
fn gfm_html_block_end_marker_may_share_the_start_line() {
    let html = render_html("<style>p{color:red;}</style>\n*foo*\n");
    assert_eq!(html, "<style>p{color:red;}</style>\n<p><em>foo</em></p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-partial-tag-need-not-be-completed/test]
#[test]
fn gfm_html_block_type_6_partial_tag_need_not_be_completed() {
    let html = render_html("<div id=\"foo\"\n*hi*\n");
    assert_eq!(html, "<div id=\"foo\"\n*hi*\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-initial-tag-need-not-be-alone-on-line/test]
#[test]
fn gfm_html_block_type_6_initial_tag_need_not_be_alone_on_line() {
    let html = render_html("<div><a href=\"bar\">*foo*</a></div>\n");
    assert_eq!(html, "<div><a href=\"bar\">*foo*</a></div>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-7-tag-name-may-be-anything/test]
#[test]
fn gfm_html_block_type_7_tag_name_may_be_anything() {
    let html = render_html("<Warning>\n*bar*\n</Warning>\n");
    assert_eq!(html, "<Warning>\n*bar*\n</Warning>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.block-vs-inline-tag-behavior/test]
#[test]
fn gfm_html_block_del_on_its_own_line_stays_a_raw_html_block() {
    let html = render_html("<del>\n*foo*\n</del>\n");
    assert_eq!(html, "<del>\n*foo*\n</del>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.blank-line-after-opening-tag-allows-following-markdown/test]
#[test]
fn gfm_html_block_blank_line_after_opening_tag_allows_markdown_content() {
    let html = render_html("<del>\n\n*foo*\n\n</del>\n");
    assert_eq!(html, "<del>\n<p><em>foo</em></p>\n</del>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.inline-html-when-tag-is-not-alone-on-line/test]
#[test]
fn gfm_html_block_inline_html_when_tag_is_not_alone_on_line() {
    let html = render_html("<del>*foo*</del>\n");
    assert_eq!(html, "<p><del><em>foo</em></del></p>\n");
}
