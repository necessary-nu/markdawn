//! Rendering tests for the Dioxus adapter, plus the shared helpers the
//! vdom-agreement suite in `dioxus_vdom_parity_tests` also uses.

use super::*;

pub(super) fn parse_nodes(source: &str, options: DioxusOptions) -> Vec<Node<()>> {
    let mut builder = Parser::<()>::builder();
    if options.gfm {
        builder = builder.with_gfm();
    }
    if options.math {
        builder = builder.with_math();
    }
    if options.alerts {
        builder = builder.with_alerts();
    }
    if options.highlights {
        builder = builder.with_highlights();
    }
    if options.definition_lists {
        builder = builder.with_definition_lists();
    }
    let mut parser = builder.build();
    parser
        .parse_full_at(source, 0)
        .unwrap()
        .into_iter()
        .map(|block| block.node)
        .collect()
}

/// Serialize nodes through the canonical vdom renderer, with the
/// `nl_open` / `nl_close` whitespace hints — which have no `rsx!`
/// counterpart — collapsed out so the two renderings are comparable.
pub(super) fn vdom_html_with(
    nodes: &[Node<()>],
    adapter: &crate::adapter::html::HtmlAdapter<()>,
) -> String {
    let mut out = String::new();
    for node in nodes {
        let item = crate::node::StreamItem::Upsert(crate::node::Emission {
            id: 0,
            provisional: false,
            node: node.clone(),
        });
        if let crate::node::StreamItem::Upsert(emission) = adapter.render(item).unwrap() {
            out.push_str(&emission.node);
        }
    }
    out.replace('\n', "")
}

pub(super) fn vdom_html(source: &str, options: DioxusOptions) -> String {
    let nodes = parse_nodes(source, options);
    vdom_html_with(&nodes, &crate::adapter::html::HtmlAdapter::new())
}

/// `(dioxus, vdom)` renderings of the same source.
pub(super) fn both(source: &str, options: DioxusOptions) -> (String, String) {
    (
        dioxus_ssr::render_element(render_markdown(source, options)),
        vdom_html(source, options),
    )
}

#[test]
fn paragraph_renders_p_tag() {
    let html = dioxus_ssr::render_element(render_markdown("hello world", DioxusOptions::default()));
    assert_eq!(html, "<p>hello world</p>");
}

#[test]
fn heading_renders_correct_tag() {
    let html =
        dioxus_ssr::render_element(render_markdown("## Subheading", DioxusOptions::default()));
    assert_eq!(html, "<h2>Subheading</h2>");
}

#[test]
fn nested_blockquote_with_heading() {
    let html = dioxus_ssr::render_element(render_markdown("> # title", DioxusOptions::default()));
    assert!(html.contains("<blockquote>"));
    assert!(html.contains("<h1>title</h1>"));
    assert!(html.contains("</blockquote>"));
}

#[test]
fn link_has_href_attribute() {
    let html = dioxus_ssr::render_element(render_markdown(
        "[click](https://example.com)",
        DioxusOptions::default(),
    ));
    assert!(html.contains("href=\"https://example.com\""));
    assert!(html.contains(">click</a>"));
}

// [spec:adv:req:markdown.url-allowlist/test]
#[test]
fn javascript_link_href_is_stripped() {
    let html = dioxus_ssr::render_element(render_markdown(
        "[x](javascript:alert(1))",
        DioxusOptions::default(),
    ));
    assert!(!html.contains("javascript:"), "got: {html}");
    assert!(html.contains("href=\"\""), "got: {html}");
}

#[test]
fn javascript_image_src_is_stripped() {
    let html = dioxus_ssr::render_element(render_markdown(
        "![x](javascript:alert(1))",
        DioxusOptions::default(),
    ));
    assert!(!html.contains("javascript:"), "got: {html}");
}

#[test]
fn data_url_in_link_is_stripped() {
    // Base64-encoded `<script>alert(1)</script>` without parens so
    // markdown parses the URL cleanly; sanitiser should still reject
    // the `data:` scheme.
    let html = dioxus_ssr::render_element(render_markdown(
        "[x](data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==)",
        DioxusOptions::default(),
    ));
    assert!(!html.contains("data:"), "got: {html}");
}

#[test]
fn vbscript_url_in_link_is_stripped() {
    let html = dioxus_ssr::render_element(render_markdown(
        "[x](vbscript:msgbox)",
        DioxusOptions::default(),
    ));
    assert!(!html.contains("vbscript:"), "got: {html}");
}

#[test]
fn code_block_uses_language_class() {
    let html = dioxus_ssr::render_element(render_markdown(
        "```rust\nlet x = 1;\n```",
        DioxusOptions::default(),
    ));
    assert!(html.contains("class=\"language-rust\""));
    assert!(html.contains("let x = 1;"));
}

// [spec:mw:def:dioxus.surface.options+1/test]
#[test]
fn alert_renders_github_class_structure() {
    let html = dioxus_ssr::render_element(render_markdown(
        "> [!WARNING]\n> careful",
        DioxusOptions {
            alerts: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("markdown-alert"));
    assert!(html.contains("markdown-alert-warning"));
    assert!(html.contains(">Warning<"));
    assert!(html.contains("careful"));
}

#[test]
fn alert_custom_title_overrides_default_label() {
    let html = dioxus_ssr::render_element(render_markdown(
        "> [!NOTE] My custom title\n> body paragraph",
        DioxusOptions {
            alerts: true,
            ..Default::default()
        },
    ));
    assert!(
        html.contains(">My custom title<"),
        "expected custom title; got: {html}"
    );
    assert!(
        !html.contains(">Note<"),
        "default label should be replaced; got: {html}"
    );
    assert!(html.contains("body paragraph"));
}

// [spec:mw:req:core.node.alert-title-fallback/test]
#[test]
fn alert_without_title_keeps_default_label() {
    let html = dioxus_ssr::render_element(render_markdown(
        "> [!NOTE]\n> body",
        DioxusOptions {
            alerts: true,
            ..Default::default()
        },
    ));
    assert!(
        html.contains(">Note<"),
        "expected default label; got: {html}"
    );
}

// [spec:mw:req:dioxus.structure.vdom-agreement+1/test]
// [spec:mw:req:core.node.table-cell-alignment/test]
#[test]
fn table_alignment_emits_text_align_style() {
    let src = "| a | b |\n|:--|---:|\n| 1 | 2 |";
    let html = dioxus_ssr::render_element(render_markdown(
        src,
        DioxusOptions {
            gfm: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("style=\"text-align: left\""));
    assert!(html.contains("style=\"text-align: right\""));
}

// [spec:mw:def:core.node.footnote-triple/test]
#[test]
fn footnote_ref_renders_sup_link() {
    let src = "see[^1]\n\n[^1]: a footnote";
    let html = dioxus_ssr::render_element(render_markdown(
        src,
        DioxusOptions {
            gfm: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("class=\"footnote-ref\""));
    assert!(html.contains("href=\"#fn-1\""));
}

#[test]
fn math_block_renders_inline_svg() {
    let html = dioxus_ssr::render_element(render_markdown(
        "$$x^2 + y^2 = z^2$$",
        DioxusOptions {
            math: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("<svg"), "expected SVG, got: {html}");
    assert!(html.contains("class=\"math math-display\""));
}

#[test]
fn math_inline_renders_inline_svg() {
    let html = dioxus_ssr::render_element(render_markdown(
        "the formula $a + b$ is simple",
        DioxusOptions {
            math: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("<svg"), "expected SVG, got: {html}");
    assert!(html.contains("class=\"math math-inline\""));
}

#[test]
fn task_list_item_includes_checkbox() {
    let html = dioxus_ssr::render_element(render_markdown(
        "- [x] done",
        DioxusOptions {
            gfm: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("type=\"checkbox\""));
    assert!(html.contains("disabled"));
    assert!(html.contains("checked"));
}

#[test]
fn tight_list_strips_paragraph_wrapper() {
    let html = dioxus_ssr::render_element(render_markdown(
        "- one\n- two\n- three",
        DioxusOptions::default(),
    ));
    // Tight list: <li>one</li>, NOT <li><p>one</p></li>
    assert!(
        !html.contains("<p>one"),
        "tight list shouldn't wrap in <p>: {html}"
    );
    assert!(html.contains("<li>one"), "got: {html}");
}

#[test]
fn loose_list_keeps_paragraph_wrapper() {
    // Blank line between items → loose list
    let html = dioxus_ssr::render_element(render_markdown(
        "- one\n\n- two\n",
        DioxusOptions::default(),
    ));
    assert!(
        html.contains("<p>one</p>"),
        "loose list should wrap in <p>: {html}"
    );
}

#[test]
fn footnote_section_wraps_in_section_and_ol() {
    let src = "see[^1]\n\n[^1]: a footnote";
    let html = dioxus_ssr::render_element(render_markdown(
        src,
        DioxusOptions {
            gfm: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("<section class=\"footnotes\""), "got: {html}");
    assert!(html.contains("data-footnotes"), "got: {html}");
    assert!(html.contains("<ol>"), "got: {html}");
    assert!(html.contains("id=\"fn-1\""), "got: {html}");
}

// [spec:mw:req:core.node.footnote-numbering/test]
#[test]
fn footnote_definition_appends_backref() {
    let src = "see[^1]\n\n[^1]: a footnote";
    let html = dioxus_ssr::render_element(render_markdown(
        src,
        DioxusOptions {
            gfm: true,
            ..Default::default()
        },
    ));
    assert!(html.contains("class=\"footnote-backref\""), "got: {html}");
    assert!(html.contains("href=\"#fnref-1\""), "got: {html}");
    assert!(html.contains("↩"), "expected backref arrow: {html}");
}

// [spec:mw:req:dioxus.html.host-element+1/test]
// [spec:mw:req:core.html-filter.default-safe/test]
#[test]
fn html_block_strips_dangerous_tags() {
    let src = "<script>alert('xss')</script>\n\nsafe";
    let html = dioxus_ssr::render_element(render_markdown(
        src,
        DioxusOptions {
            gfm: true,
            ..Default::default()
        },
    ));
    // The tagfilter replaces `<` with `&lt;` for disallowed tags
    assert!(
        !html.contains("<script>") || html.contains("&lt;script"),
        "expected script tag filtered, got: {html}"
    );
}

#[test]
fn unrestricted_filter_passes_script_through() {
    let src = "<script>foo</script>";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_html_filter(crate::HtmlFilter::unrestricted())
            .render(src),
    );
    // Dioxus's dangerous_inner_html doesn't escape, but the SSR output
    // still HTML-escapes the result of dangerous_inner_html attribute
    // setting. Either way, the tag should NOT be neutered (no &lt;script).
    assert!(!html.contains("&lt;script"), "got: {html}");
}

#[cfg(feature = "pikchr")]
#[test]
fn pikchr_block_renders_inline_svg() {
    let src = "```pikchr\nbox \"Hello\"\n```";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                pikchr: true,
                ..Default::default()
            })
            .render(src),
    );
    assert!(html.contains("class=\"pikchr\""), "got: {html}");
    assert!(html.contains("<svg"), "got: {html}");
}

#[cfg(feature = "pikchr")]
#[test]
fn pikchr_block_falls_back_when_disabled() {
    let src = "```pikchr\nbox \"Hello\"\n```";
    let html = dioxus_ssr::render_element(render_markdown(src, DioxusOptions::default()));
    // pikchr feature off in options → just a code block
    assert!(html.contains("<pre>"));
    assert!(!html.contains("<svg"));
}

#[cfg(feature = "graphviz")]
#[test]
fn graphviz_block_renders_inline_svg() {
    let src = "```dot\ndigraph { A -> B }\n```";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src),
    );
    assert!(html.contains("class=\"graphviz\""), "got: {html}");
    assert!(html.contains("<svg"), "got: {html}");
}

#[cfg(feature = "graphviz")]
#[test]
fn graphviz_block_falls_back_when_disabled() {
    let src = "```dot\ndigraph { A -> B }\n```";
    let html = dioxus_ssr::render_element(render_markdown(src, DioxusOptions::default()));
    assert!(html.contains("<pre>"));
    assert!(!html.contains("<svg"));
}

#[cfg(feature = "graphviz")]
#[test]
fn graphviz_info_string_alias_also_renders() {
    let src = "```graphviz\ndigraph { A -> B }\n```";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src),
    );
    assert!(html.contains("class=\"graphviz\""), "got: {html}");
    assert!(html.contains("<svg"), "got: {html}");
}

#[cfg(feature = "graphviz")]
#[test]
fn graphviz_javascript_href_sanitized_in_output_svg() {
    let src = "```dot\ndigraph { A [href=\"javascript:alert(1)\"] }\n```";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src),
    );
    assert!(
        html.contains("class=\"graphviz\""),
        "no graphviz div: {html}"
    );
    assert!(!html.contains("javascript:"), "leaked javascript: {html}");
}

#[cfg(feature = "graphviz")]
#[test]
fn graphviz_url_alias_javascript_sanitized() {
    // `URL=` is an alias for `href=`. Graphviz emits it as an
    // xlink:href in SVG, where the post-render sanitiser catches
    // the unsafe scheme.
    let src = "```dot\ndigraph { A [URL=\"javascript:alert(1)\"]; B }\n```";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src),
    );
    assert!(!html.contains("javascript:"), "leaked: {html}");
}

#[cfg(feature = "graphviz")]
#[test]
fn graphviz_html_label_img_path_does_not_leak() {
    let src = "```dot\ndigraph { A [label=<<IMG SRC=\"/etc/passwd\"/>>] }\n```";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src),
    );
    assert!(!html.contains("/etc/passwd"), "leaked: {html}");
}

#[cfg(feature = "graphviz")]
#[test]
fn graphviz_safe_href_preserved_in_output_svg() {
    let src = "```dot\ndigraph { A [href=\"https://example.com\"] }\n```";
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions {
                graphviz: true,
                ..Default::default()
            })
            .render(src),
    );
    assert!(
        html.contains("https://example.com"),
        "link stripped: {html}"
    );
}

#[cfg(feature = "highlight")]
#[test]
fn highlighter_path_invoked_for_known_language() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct StubHl(Rc<Cell<bool>>);
    impl crate::highlight::SyntaxHighlighter for StubHl {
        fn highlight(
            &mut self,
            _lang: &str,
            _src: &str,
        ) -> Result<Vec<crate::SyntaxSpan>, crate::HighlightError> {
            self.0.set(true);
            Err(crate::HighlightError::new("no grammar"))
        }
    }

    // [spec:gfm:req:markdawn.highlighting.boundary.error-fallback/test]
    let invoked = Rc::new(Cell::new(false));
    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_highlighter(StubHl(invoked.clone()))
            .render("```rust\nfoo\n```"),
    );
    assert!(
        invoked.get(),
        "highlighter should be queried for `rust` block"
    );
    assert!(html.contains("class=\"language-rust\""), "got: {html}");
    assert!(html.contains("foo"), "plain fallback lost source: {html}");
}

#[cfg(feature = "highlight")]
#[test]
fn highlighter_skipped_for_unknown_language_with_no_info() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct StubHl(Rc<Cell<u32>>);
    impl crate::highlight::SyntaxHighlighter for StubHl {
        fn highlight(
            &mut self,
            _lang: &str,
            _src: &str,
        ) -> Result<Vec<crate::SyntaxSpan>, crate::HighlightError> {
            self.0.set(self.0.get() + 1);
            Err(crate::HighlightError::new("unexpected call"))
        }
    }

    let calls = Rc::new(Cell::new(0));
    let _ = dioxus_ssr::render_element(
        Renderer::default()
            .with_highlighter(StubHl(calls.clone()))
            .render("```\nno-lang\n```"),
    );
    assert_eq!(calls.get(), 0, "no info string → highlighter not called");
}

// [spec:mw:def:core.rewriter.types/test]
#[test]
fn code_block_rewriter_substitutes_info_and_literal() {
    struct StubRewriter;
    impl crate::ext::CodeBlockRewriter for StubRewriter {
        fn rewrite(
            &self,
            info: Option<&str>,
            _literal: &str,
        ) -> Option<crate::ext::CodeBlockRewrite> {
            if info == Some("foo") {
                Some(crate::ext::CodeBlockRewrite {
                    info: Some("bar".into()),
                    literal: "hello world".into(),
                })
            } else {
                None
            }
        }
    }

    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_code_block_rewriter(StubRewriter)
            .render("```foo\nignored content\n```"),
    );
    assert!(
        html.contains("class=\"language-bar\""),
        "info should be substituted; got: {html}"
    );
    assert!(
        html.contains("hello world"),
        "literal should be substituted; got: {html}"
    );
    assert!(
        !html.contains("ignored content"),
        "original literal should not appear; got: {html}"
    );
}

// [spec:mw:req:core.rewriter.dispatch-order/test]
#[test]
fn code_block_rewriter_passes_through_non_matching() {
    struct NeverMatches;
    impl crate::ext::CodeBlockRewriter for NeverMatches {
        fn rewrite(
            &self,
            _info: Option<&str>,
            _literal: &str,
        ) -> Option<crate::ext::CodeBlockRewrite> {
            None
        }
    }

    let html = dioxus_ssr::render_element(
        Renderer::default()
            .with_code_block_rewriter(NeverMatches)
            .render("```text\nfoo\n```"),
    );
    assert!(html.contains("class=\"language-text\""), "got: {html}");
    assert!(html.contains("foo"), "got: {html}");
}
