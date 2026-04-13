use darwin_markdown::{Node, Parser, StreamItem, TableAlignment, VecSink};

fn parse_final_nodes(input: &str) -> Vec<Node<(), ()>> {
    let mut parser = Parser::<(), ()>::new();
    let mut sink = VecSink::new();
    parser.push_str(input, &mut sink).unwrap();
    parser.finish(&mut sink).unwrap();

    let mut order = Vec::new();
    let mut values = Vec::<(u64, Node<(), ()>)>::new();

    for item in sink.items() {
        match item {
            StreamItem::Upsert(emission) => {
                if let Some((_, node)) = values.iter_mut().find(|(id, _)| *id == emission.id) {
                    *node = emission.node.clone();
                } else {
                    order.push(emission.id);
                    values.push((emission.id, emission.node.clone()));
                }
            }
            StreamItem::Delete { id } => {
                order.retain(|existing| existing != id);
                values.retain(|(existing, _)| existing != id);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|id| values.iter().find(|(existing, _)| *existing == id))
        .map(|(_, node)| node.clone())
        .collect()
}

fn render_html(nodes: &[Node<(), ()>]) -> String {
    let mut out = String::new();
    for node in nodes {
        render_block(node, &mut out);
    }
    out
}

fn render_block(node: &Node<(), ()>, out: &mut String) {
    match node {
        Node::Paragraph { children, .. } => {
            out.push_str("<p>");
            render_inlines(children, out);
            out.push_str("</p>\n");
        }
        Node::Heading {
            level, children, ..
        } => {
            out.push_str("<h");
            out.push(char::from(b'0' + *level));
            out.push('>');
            render_inlines(children, out);
            out.push_str("</h");
            out.push(char::from(b'0' + *level));
            out.push_str(">\n");
        }
        Node::CodeBlock { info, literal, .. } => {
            match info {
                Some(info) => {
                    out.push_str("<pre><code class=\"language-");
                    escape_html(info, out);
                    out.push_str("\">");
                }
                None => out.push_str("<pre><code>"),
            }
            escape_html(literal, out);
            out.push_str("</code></pre>\n");
        }
        Node::BlockQuote { children, .. } => {
            out.push_str("<blockquote>\n");
            for child in children {
                render_block(child, out);
            }
            out.push_str("</blockquote>\n");
        }
        Node::List {
            ordered,
            start,
            tight,
            children,
            ..
        } => {
            if *ordered {
                match start {
                    Some(1) | None => out.push_str("<ol>\n"),
                    Some(start) => {
                        out.push_str("<ol start=\"");
                        out.push_str(&start.to_string());
                        out.push_str("\">\n");
                    }
                }
            } else {
                out.push_str("<ul>\n");
            }
            for child in children {
                render_list_child(child, tight.unwrap_or(true), out);
            }
            if *ordered {
                out.push_str("</ol>\n");
            } else {
                out.push_str("</ul>\n");
            }
        }
        Node::Item { .. } | Node::TaskItem { .. } => render_list_child(node, true, out),
        Node::Table {
            alignments,
            header,
            rows,
            ..
        } => {
            out.push_str("<table>\n<thead>\n");
            for row in header {
                render_table_row(row, alignments, out);
            }
            out.push_str("</thead>\n<tbody>\n");
            for row in rows {
                render_table_row(row, alignments, out);
            }
            out.push_str("</tbody>\n</table>\n");
        }
        Node::TableRow { .. } | Node::TableCell { .. } => render_table_row(node, &[], out),
        Node::ThematicBreak { .. } => out.push_str("<hr />\n"),
        Node::Html { html, .. } => {
            out.push_str(html);
            if !html.ends_with('\n') {
                out.push('\n');
            }
        }
        other => panic!("unsupported block in test renderer: {other:?}"),
    }
}

fn render_inlines(nodes: &[Node<(), ()>], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text { text, .. } => escape_html(text, out),
            Node::Emphasis { children, .. } => {
                out.push_str("<em>");
                render_inlines(children, out);
                out.push_str("</em>");
            }
            Node::Strong { children, .. } => {
                out.push_str("<strong>");
                render_inlines(children, out);
                out.push_str("</strong>");
            }
            Node::Strikethrough { children, .. } => {
                out.push_str("<del>");
                render_inlines(children, out);
                out.push_str("</del>");
            }
            Node::Link {
                destination,
                title,
                children,
                ..
            } => {
                out.push_str("<a href=\"");
                escape_html(destination, out);
                out.push('"');
                if let Some(title) = title {
                    out.push_str(" title=\"");
                    escape_html(title, out);
                    out.push('"');
                }
                out.push('>');
                render_inlines(children, out);
                out.push_str("</a>");
            }
            Node::Image {
                destination,
                title,
                alt,
                ..
            } => {
                out.push_str("<img src=\"");
                escape_html(destination, out);
                out.push_str("\" alt=\"");
                escape_html(alt, out);
                out.push('"');
                if let Some(title) = title {
                    out.push_str(" title=\"");
                    escape_html(title, out);
                    out.push('"');
                }
                out.push_str(" />");
            }
            Node::CodeSpan { code, .. } => {
                out.push_str("<code>");
                escape_html(code, out);
                out.push_str("</code>");
            }
            Node::Html { html, .. } => out.push_str(html),
            Node::HardBreak { .. } => out.push_str("<br />\n"),
            Node::SoftBreak { .. } => out.push('\n'),
            other => panic!("unsupported inline in test renderer: {other:?}"),
        }
    }
}

fn render_table_row(node: &Node<(), ()>, alignments: &[Option<TableAlignment>], out: &mut String) {
    let Node::TableRow { cells, .. } = node else {
        return;
    };
    out.push_str("<tr>");
    for (index, cell) in cells.iter().enumerate() {
        let Node::TableCell {
            children,
            header,
            alignment,
            ..
        } = cell
        else {
            continue;
        };
        let tag = if *header { "th" } else { "td" };
        out.push('<');
        out.push_str(tag);
        let align = alignment.or_else(|| alignments.get(index).copied().flatten());
        if let Some(align) = align {
            out.push_str(" align=\"");
            out.push_str(match align {
                TableAlignment::Left => "left",
                TableAlignment::Center => "center",
                TableAlignment::Right => "right",
            });
            out.push('"');
        }
        out.push('>');
        render_inlines(children, out);
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    }
    out.push_str("</tr>\n");
}

fn render_list_child(node: &Node<(), ()>, tight: bool, out: &mut String) {
    match node {
        Node::Item { children, .. } => {
            out.push_str("<li>");
            if children.is_empty() {
                out.push_str("</li>\n");
            } else if tight && matches!(children.first(), Some(Node::Paragraph { .. })) {
                if let Node::Paragraph { children, .. } = &children[0] {
                    render_inlines(children, out);
                }
                for child in &children[1..] {
                    render_block(child, out);
                }
                out.push_str("</li>\n");
            } else {
                out.push('\n');
                for child in children {
                    render_block(child, out);
                }
                out.push_str("</li>\n");
            }
        }
        Node::TaskItem {
            checked, children, ..
        } => {
            out.push_str("<li><input type=\"checkbox\" disabled=\"\" ");
            if *checked {
                out.push_str("checked=\"\" ");
            }
            out.push_str("/> ");
            if children.is_empty() {
                out.push_str("</li>\n");
            } else if tight && matches!(children.first(), Some(Node::Paragraph { .. })) {
                if let Node::Paragraph { children, .. } = &children[0] {
                    render_inlines(children, out);
                }
                for child in &children[1..] {
                    render_block(child, out);
                }
                out.push_str("</li>\n");
            } else {
                out.push('\n');
                for child in children {
                    render_block(child, out);
                }
                out.push_str("</li>\n");
            }
        }
        other => render_block(other, out),
    }
}

fn escape_html(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

#[test]
fn gfm_example_13_thematic_breaks() {
    let html = render_html(&parse_final_nodes("***\n---\n___\n"));
    assert_eq!(html, "<hr />\n<hr />\n<hr />\n");
}

#[test]
fn gfm_example_32_atx_headings() {
    let html = render_html(&parse_final_nodes(
        "# foo\n## foo\n### foo\n#### foo\n##### foo\n###### foo\n",
    ));
    assert_eq!(
        html,
        "<h1>foo</h1>\n<h2>foo</h2>\n<h3>foo</h3>\n<h4>foo</h4>\n<h5>foo</h5>\n<h6>foo</h6>\n"
    );
}

#[test]
fn gfm_example_80_setext_headings() {
    let html = render_html(&parse_final_nodes(
        "Foo *bar*\n=========\n\nFoo *bar*\n---------\n",
    ));
    assert_eq!(
        html,
        "<h1>Foo <em>bar</em></h1>\n<h2>Foo <em>bar</em></h2>\n"
    );
}

#[test]
fn gfm_example_126_unclosed_fence_at_eof() {
    let html = render_html(&parse_final_nodes("```\n"));
    assert_eq!(html, "<pre><code></code></pre>\n");
}

#[test]
fn gfm_backslash_hard_break() {
    let html = render_html(&parse_final_nodes("foo\\\nbar\n"));
    assert_eq!(html, "<p>foo<br />\nbar</p>\n");
}

#[test]
fn gfm_code_span() {
    let html = render_html(&parse_final_nodes("`foo`\n"));
    assert_eq!(html, "<p><code>foo</code></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.single-asterisk-can-open-emphasis]
// gfm[verify inlines.emphasis-and-strong-emphasis.single-asterisk-can-close-emphasis]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-can-open-strong-emphasis]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-can-close-strong-emphasis]
#[test]
fn gfm_emphasis_and_strong() {
    let html = render_html(&parse_final_nodes("*em* and **strong**\n"));
    assert_eq!(html, "<p><em>em</em> and <strong>strong</strong></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.emphasis-is-not-allowed-inside-words]
#[test]
fn gfm_intraword_underscore_is_literal() {
    let html = render_html(&parse_final_nodes("foo_bar_baz\n"));
    assert_eq!(html, "<p>foo_bar_baz</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-permitted]
#[test]
fn gfm_intraword_asterisk_is_allowed() {
    let html = render_html(&parse_final_nodes("foo*bar*baz\n"));
    assert_eq!(html, "<p>foo<em>bar</em>baz</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.underscore-can-open-when-punctuation-precedes]
// gfm[verify inlines.emphasis-and-strong-emphasis.single-underscore-can-open-emphasis]
// gfm[verify inlines.emphasis-and-strong-emphasis.single-underscore-can-close-emphasis]
#[test]
fn gfm_punctuation_gated_underscore_emphasis() {
    let html = render_html(&parse_final_nodes("foo-_(bar)_\n"));
    assert_eq!(html, "<p>foo-<em>(bar)</em></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.underscore-can-close-when-punctuation-follows]
#[test]
fn gfm_punctuation_gated_underscore_closer() {
    let html = render_html(&parse_final_nodes("_(bar)_.\n"));
    assert_eq!(html, "<p><em>(bar)</em>.</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-produces-nested-strong]
#[test]
fn gfm_triple_asterisk_nests_emphasis_and_strong() {
    let html = render_html(&parse_final_nodes("***foo***\n"));
    assert_eq!(html, "<p><em><strong>foo</strong></em></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.both-multiples-of-three-allow-emphasis-match]
#[test]
fn gfm_multiple_of_three_balanced_case() {
    let html = render_html(&parse_final_nodes("foo***bar***baz\n"));
    assert_eq!(html, "<p>foo<em><strong>bar</strong></em>baz</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.strong-emphasis-requires-matching-delimiters]
#[test]
fn gfm_nested_strong_inside_emphasis() {
    let html = render_html(&parse_final_nodes("*foo **bar***\n"));
    assert_eq!(html, "<p><em>foo <strong>bar</strong></em></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-permitted]
#[test]
fn gfm_intraword_double_asterisk_is_allowed() {
    let html = render_html(&parse_final_nodes("foo**bar**baz\n"));
    assert_eq!(html, "<p>foo<strong>bar</strong>baz</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-forbidden]
#[test]
fn gfm_intraword_double_underscore_is_literal() {
    let html = render_html(&parse_final_nodes("foo__bar__baz\n"));
    assert_eq!(html, "<p>foo__bar__baz</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-can-open-when-punctuation-precedes]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-can-open-strong-emphasis]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-can-close-strong-emphasis]
#[test]
fn gfm_punctuation_gated_double_underscore_strong() {
    let html = render_html(&parse_final_nodes("foo-__(bar)__\n"));
    assert_eq!(html, "<p>foo-<strong>(bar)</strong></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-can-close-when-punctuation-follows]
#[test]
fn gfm_punctuation_gated_double_underscore_closer() {
    let html = render_html(&parse_final_nodes("__(bar)__.\n"));
    assert_eq!(html, "<p><strong>(bar)</strong>.</p>\n");
}

// gfm[verify inlines.links.inline-link-definition]
#[test]
fn gfm_inline_link() {
    let html = render_html(&parse_final_nodes("[link](/dest \"title\")\n"));
    assert_eq!(html, "<p><a href=\"/dest\" title=\"title\">link</a></p>\n");
}

// gfm[verify inlines.images.definition]
#[test]
fn gfm_inline_image() {
    let html = render_html(&parse_final_nodes("![alt](/img.png)\n"));
    assert_eq!(html, "<p><img src=\"/img.png\" alt=\"alt\" /></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.may-appear-before-or-after-uses]
#[test]
fn gfm_reference_link_later_definition() {
    let html = render_html(&parse_final_nodes("[foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// gfm[verify inlines.raw-html.html-like-tag-text-is-parsed-as-raw-html]
#[test]
fn gfm_inline_raw_html() {
    let html = render_html(&parse_final_nodes("a <span>hi</span> b\n"));
    assert_eq!(html, "<p>a <span>hi</span> b</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.definition]
#[test]
fn gfm_html_block_passthrough() {
    let html = render_html(&parse_final_nodes("<div>\nfoo\n</div>\n"));
    assert_eq!(html, "<div>\nfoo\n</div>\n");
}

// gfm[verify inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
#[test]
fn gfm_strikethrough() {
    let html = render_html(&parse_final_nodes("~~gone~~\n"));
    assert_eq!(html, "<p><del>gone</del></p>\n");
}

// gfm[verify inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
#[test]
fn gfm_strikethrough_skips_internal_code_span_closers() {
    let html = render_html(&parse_final_nodes("~~`code ~~`~~\n"));
    assert_eq!(html, "<p><del><code>code ~~</code></del></p>\n");
}

// gfm[verify inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
#[test]
fn gfm_strikethrough_rejects_whitespace_delimited_cases() {
    let html = render_html(&parse_final_nodes("~~ hi~~ and ~~hi ~~\n"));
    assert_eq!(html, "<p>~~ hi~~ and ~~hi ~~</p>\n");
}

// gfm[verify inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
#[test]
fn gfm_strikethrough_spans_soft_breaks() {
    let html = render_html(&parse_final_nodes("This ~~has a\nsoft break~~.\n"));
    assert_eq!(html, "<p>This <del>has a\nsoft break</del>.</p>\n");
}

// gfm[verify inlines.strikethrough-extension.new-paragraph-ends-strikethrough-parse]
#[test]
fn gfm_strikethrough_stops_at_paragraph_breaks() {
    let html = render_html(&parse_final_nodes("This ~~has a\n\nnew paragraph~~.\n"));
    assert_eq!(html, "<p>This ~~has a</p>\n<p>new paragraph~~.</p>\n");
}

// gfm[verify container-blocks.task-list-items-extension.definition]
#[test]
fn gfm_task_list_item() {
    let html = render_html(&parse_final_nodes("- [x] done\n- [ ] todo\n"));
    assert_eq!(
        html,
        "<ul>\n<li><input type=\"checkbox\" disabled=\"\" checked=\"\" /> done</li>\n<li><input type=\"checkbox\" disabled=\"\" /> todo</li>\n</ul>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.table-definition]
#[test]
fn gfm_table() {
    let html = render_html(&parse_final_nodes("| a | b |\n| :-- | --: |\n| 1 | 2 |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th align=\"left\">a</th><th align=\"right\">b</th></tr>\n</thead>\n<tbody>\n<tr><td align=\"left\">1</td><td align=\"right\">2</td></tr>\n</tbody>\n</table>\n"
    );
}

// gfm[verify inlines.autolinks-extension.extended-url-autolink-definition]
#[test]
fn gfm_autolink_literal() {
    let html = render_html(&parse_final_nodes("visit https://example.com now\n"));
    assert_eq!(
        html,
        "<p>visit <a href=\"https://example.com\">https://example.com</a> now</p>\n"
    );
}

// gfm[verify container-blocks.list-items.lazy-continuation-example]
#[test]
fn gfm_loose_list_and_lazy_continuation() {
    let html = render_html(&parse_final_nodes(
        "- one\n  still one\n\n- two\n\n  two para\n",
    ));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>one\nstill one</p>\n</li>\n<li>\n<p>two</p>\n<p>two para</p>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.ordered-start-number-is-at-most-nine-digits]
// gfm[verify container-blocks.list-items.ordered-start-number-may-have-leading-zeroes]
#[test]
fn gfm_ordered_marker_width_rules() {
    let html = render_html(&parse_final_nodes(
        "123456789. ok\n\n1234567890. not ok\n\n003. also ok\n",
    ));
    assert_eq!(
        html,
        "<ol start=\"123456789\">\n<li>ok</li>\n</ol>\n<p>1234567890. not ok</p>\n<ol start=\"3\">\n<li>also ok</li>\n</ol>\n"
    );
}

// gfm[verify container-blocks.list-items.interrupting-ordered-list-must-start-at-one]
#[test]
fn gfm_ordered_list_interrupt_requires_one() {
    let html = render_html(&parse_final_nodes("para\n2. no interrupt\n"));
    assert_eq!(html, "<p>para\n2. no interrupt</p>\n");
}

// gfm[verify container-blocks.list-items.empty-item-does-not-require-post-marker-spaces]
#[test]
fn gfm_empty_list_item_without_space() {
    let html = render_html(&parse_final_nodes("*\n* next\n"));
    assert_eq!(html, "<ul>\n<li></li>\n<li>next</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.interrupting-first-item-cannot-begin-with-blank-line]
// gfm[verify container-blocks.list-items.blank-line-start-makes-post-marker-spacing-irrelevant]
#[test]
fn gfm_blank_start_list_item_cases() {
    let html = render_html(&parse_final_nodes("para\n+\n  foo\n"));
    assert_eq!(html, "<p>para\n+\n  foo</p>\n");

    let html = render_html(&parse_final_nodes("+   \n  foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.sublists-follow-general-indentation-rules]
#[test]
fn gfm_nested_sublist() {
    let html = render_html(&parse_final_nodes("- one\n  - two\n"));
    assert_eq!(
        html,
        "<ul>\n<li>one<ul>\n<li>two</li>\n</ul>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.indented-code-must-be-four-spaces-beyond-content-indent]
#[test]
fn gfm_indented_code_inside_list_item() {
    let html = render_html(&parse_final_nodes("- foo\n\n      bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>bar</code></pre>\n</li>\n</ul>\n"
    );
}

// gfm[verify preliminaries.tabs.tab-indented-list-continuation-equivalence]
// gfm[verify preliminaries.tabs.tab-indented-code-block-equivalence]
#[test]
fn gfm_tab_indentation_structural_cases() {
    let html = render_html(&parse_final_nodes("\tcode\n"));
    assert_eq!(html, "<pre><code>code</code></pre>\n");

    let html = render_html(&parse_final_nodes("-\tfoo\n\tbar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("- foo\n\n\t\tbar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>bar</code></pre>\n</li>\n</ul>\n"
    );
}

// gfm[verify preliminaries.tabs.tab-after-block-quote-marker-counts-toward-indent]
#[test]
fn gfm_tab_after_block_quote_marker_counts_toward_indent() {
    let html = render_html(&parse_final_nodes(">\t\tfoo\n"));
    assert_eq!(
        html,
        "<blockquote>\n<pre><code>  foo</code></pre>\n</blockquote>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-1-script-pre-style-start-and-end-conditions]
// gfm[verify leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs]
#[test]
fn gfm_html_block_type_1_interrupts_paragraph_and_closes_on_end_tag() {
    let html = render_html(&parse_final_nodes(
        "before\n<script>\n*not emphasis*\n</script>\nafter\n",
    ));
    assert_eq!(
        html,
        "<p>before</p>\n&lt;script>\n*not emphasis*\n&lt;/script>\n<p>after</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-2-comment-start-and-end-conditions]
#[test]
fn gfm_html_block_type_2_comment_runs_until_comment_close() {
    let html = render_html(&parse_final_nodes("<!--\n# not a heading\n-->\nafter\n"));
    assert_eq!(html, "<!--\n# not a heading\n-->\n<p>after</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.trailing-text-after-end-marker-stays-inside-block]
#[test]
fn gfm_html_block_type_1_keeps_trailing_text_on_end_marker_line_inside_block() {
    let html = render_html(&parse_final_nodes(
        "<script>\nfoo\n</script>1. *bar*\nafter\n",
    ));
    assert_eq!(
        html,
        "&lt;script>\nfoo\n&lt;/script>1. *bar*\n<p>after</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-3-processing-instruction-start-and-end-conditions]
// gfm[verify leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs]
#[test]
fn gfm_html_block_type_3_interrupts_paragraph_and_allows_blank_lines_until_close() {
    let html = render_html(&parse_final_nodes(
        "before\n<?php\n\n  echo '>';\n\n?>\nafter\n",
    ));
    assert_eq!(
        html,
        "<p>before</p>\n<?php\n\n  echo '>';\n\n?>\n<p>after</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-4-declaration-start-and-end-conditions]
// gfm[verify leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs]
#[test]
fn gfm_html_block_type_4_interrupts_paragraph_and_closes_on_gt() {
    let html = render_html(&parse_final_nodes("before\n<!DOCTYPE html>\nafter\n"));
    assert_eq!(html, "<p>before</p>\n<!DOCTYPE html>\n<p>after</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-5-cdata-start-and-end-conditions]
// gfm[verify leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs]
#[test]
fn gfm_html_block_type_5_interrupts_paragraph_and_allows_blank_lines_until_close() {
    let html = render_html(&parse_final_nodes(
        "before\n<![CDATA[\nline one\n\nline two\n]]>\nafter\n",
    ));
    assert_eq!(
        html,
        "<p>before</p>\n<![CDATA[\nline one\n\nline two\n]]>\n<p>after</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-6-block-tag-start-and-blank-line-end]
// gfm[verify leaf-blocks.html-blocks.blank-line-ends-type-6-block-and-normal-parsing-resumes]
#[test]
fn gfm_html_block_type_6_interrupts_paragraph_and_ends_at_blank_line() {
    let html = render_html(&parse_final_nodes("before\n<div>\nraw\n\nafter\n"));
    assert_eq!(html, "<p>before</p>\n<div>\nraw\n<p>after</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-6-continues-through-apparent-markdown-until-blank-line]
// gfm[verify leaf-blocks.html-blocks.type-6-and-7-usually-require-following-blank-line]
#[test]
fn gfm_html_block_type_6_keeps_following_markdown_inside_block_without_blank_line() {
    let html = render_html(&parse_final_nodes("<div>\nbar\n</div>\n*foo*\n"));
    assert_eq!(html, "<div>\nbar\n</div>\n*foo*\n");
}

// gfm[verify leaf-blocks.html-blocks.type-7-complete-tag-start-and-blank-line-end]
#[test]
fn gfm_html_block_type_7_generic_tag_at_top_level_ends_at_blank_line() {
    let html = render_html(&parse_final_nodes("<span>\nraw\n\nafter\n"));
    assert_eq!(html, "<span>\nraw\n<p>after</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.non-type-6-tag-must-be-complete-and-alone-on-first-line]
#[test]
fn gfm_html_block_type_7_requires_a_complete_non_block_tag_on_its_first_line() {
    let html = render_html(&parse_final_nodes("<a href=\"foo\">\n*bar*\n</a>\n"));
    assert_eq!(html, "<a href=\"foo\">\n*bar*\n</a>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-7-cannot-interrupt-paragraphs]
#[test]
fn gfm_html_block_type_7_does_not_interrupt_paragraph() {
    let html = render_html(&parse_final_nodes("before\n<span>\nafter\n"));
    assert_eq!(html, "<p>before\n<span>\nafter</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.block-vs-inline-tag-behavior]
// gfm[verify leaf-blocks.html-blocks.inline-html-when-tag-is-not-alone-on-line]
#[test]
fn gfm_html_block_del_tag_distinguishes_block_and_inline_forms() {
    let html = render_html(&parse_final_nodes("<del>\n*foo*\n</del>\n"));
    assert_eq!(html, "<del>\n*foo*\n</del>\n");

    let html = render_html(&parse_final_nodes("<del>*foo*</del>\n"));
    assert_eq!(html, "<p><del><em>foo</em></del></p>\n");
}

// gfm[verify leaf-blocks.html-blocks.blank-line-after-opening-tag-allows-following-markdown]
// gfm[verify leaf-blocks.html-blocks.blank-line-separates-two-html-blocks-with-markdown-between]
#[test]
fn gfm_html_block_blank_lines_can_end_one_block_and_resume_markdown() {
    let html = render_html(&parse_final_nodes("<del>\n\n*foo*\n\n</del>\n"));
    assert_eq!(html, "<del>\n<p><em>foo</em></p>\n</del>\n");

    let html = render_html(&parse_final_nodes(
        "<DIV CLASS=\"foo\">\n\n*Markdown*\n\n</DIV>\n",
    ));
    assert_eq!(
        html,
        "<DIV CLASS=\"foo\">\n<p><em>Markdown</em></p>\n</DIV>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.types-1-through-6-need-not-be-preceded-by-blank-line]
#[test]
fn gfm_html_block_type_6_interrupts_without_preceding_blank_line() {
    let html = render_html(&parse_final_nodes("Foo\n<div>\nbar\n</div>\n"));
    assert_eq!(html, "<p>Foo</p>\n<div>\nbar\n</div>\n");
}

// gfm[verify leaf-blocks.html-blocks.end-marker-may-occur-on-same-line-as-start]
// gfm[verify leaf-blocks.html-blocks.single-line-block-when-start-and-end-both-match]
// gfm[verify leaf-blocks.html-blocks.trailing-text-after-end-marker-stays-inside-block]
#[test]
fn gfm_html_block_same_line_end_markers_keep_their_line_inside_the_block() {
    let html = render_html(&parse_final_nodes("<style>p{color:red;}</style>\n*foo*\n"));
    assert_eq!(
        html,
        "&lt;style>p{color:red;}&lt;/style>\n<p><em>foo</em></p>\n"
    );

    let html = render_html(&parse_final_nodes("<!-- foo -->*bar*\n*baz*\n"));
    assert_eq!(html, "<!-- foo -->*bar*\n<p><em>baz</em></p>\n");
}

#[test]
fn gfm_example_228_block_quote() {
    let html = render_html(&parse_final_nodes("> # Foo\n> bar\n> baz\n"));
    assert_eq!(
        html,
        "<blockquote>\n<h1>Foo</h1>\n<p>bar\nbaz</p>\n</blockquote>\n"
    );
}

#[test]
fn gfm_simple_list() {
    let html = render_html(&parse_final_nodes("- one\n- two\n"));
    assert_eq!(html, "<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n");
}

#[test]
fn gfm_example_107_indented_code() {
    let html = render_html(&parse_final_nodes(
        "    a simple\n      indented code block\n",
    ));
    assert_eq!(
        html,
        "<pre><code>a simple\n  indented code block</code></pre>\n"
    );
}

// gfm[verify inlines.code-spans.all-space-content-is-not-trimmed]
#[test]
fn gfm_code_span_all_spaces_not_trimmed() {
    let html = render_html(&parse_final_nodes("`` ` ``\n"));
    assert_eq!(html, "<p><code>`</code></p>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.content-removes-up-to-opening-indent]
// gfm[verify leaf-blocks.fenced-code-blocks.fences-may-be-indented-and-content-deindents-to-match]
// gfm[verify leaf-blocks.fenced-code-blocks.short-indented-content-lines-remove-all-indent]
#[test]
fn gfm_fenced_code_deindent() {
    let html = render_html(&parse_final_nodes("  ```\n  aaa\n aaa\naaa\n  ```\n"));
    assert_eq!(html, "<pre><code>aaa\naaa\naaa</code></pre>\n");
}

// gfm[verify inlines.links.balanced-parentheses-are-allowed-in-destination]
#[test]
fn gfm_link_balanced_parens() {
    let html = render_html(&parse_final_nodes("[link](/url(test))\n"));
    assert_eq!(html, "<p><a href=\"/url(test)\">link</a></p>\n");
}

// gfm[verify inlines.links.destination-may-contain-spaces-only-in-angle-brackets]
#[test]
fn gfm_link_angle_bracket_dest() {
    let html = render_html(&parse_final_nodes("[link](<url with spaces>)\n"));
    assert_eq!(html, "<p><a href=\"url with spaces\">link</a></p>\n");
}

// gfm[verify inlines.links.destination-and-title-may-both-be-omitted]
#[test]
fn gfm_empty_inline_link() {
    let html = render_html(&parse_final_nodes("[link]()\n"));
    assert_eq!(html, "<p><a href=\"\">link</a></p>\n");
}

// gfm[verify inlines.autolinks-extension.extended-www-autolink-definition]
// gfm[verify inlines.autolinks-extension.extended-www-autolinks-insert-http-scheme]
#[test]
fn gfm_www_autolink() {
    let html = render_html(&parse_final_nodes("visit www.example.com now\n"));
    assert_eq!(
        html,
        "<p>visit <a href=\"http://www.example.com\">www.example.com</a> now</p>\n"
    );
}

// gfm[verify inlines.autolinks-extension.trailing-punctuation-is-excluded-from-autolinks]
#[test]
fn gfm_autolink_trailing_punctuation() {
    let html = render_html(&parse_final_nodes("visit https://example.com.\n"));
    assert_eq!(
        html,
        "<p>visit <a href=\"https://example.com\">https://example.com</a>.</p>\n"
    );
}

// gfm[verify inlines.autolinks-extension.unmatched-trailing-right-parentheses-are-excluded]
// gfm[verify inlines.autolinks-extension.trailing-parenthesis-check-applies-only-at-link-end]
#[test]
fn gfm_autolink_trailing_parens() {
    let html = render_html(&parse_final_nodes(
        "see https://en.wikipedia.org/wiki/Foo_(bar).\n",
    ));
    assert_eq!(
        html,
        "<p>see <a href=\"https://en.wikipedia.org/wiki/Foo_(bar)\">https://en.wikipedia.org/wiki/Foo_(bar)</a>.</p>\n"
    );
}

// gfm[verify inlines.disallowed-raw-html-extension.disallowed-tags]
// gfm[verify inlines.disallowed-raw-html-extension.filtering-replaces-leading-angle-bracket]
// gfm[verify inlines.disallowed-raw-html-extension.all-other-html-tags-are-left-untouched]
#[test]
fn gfm_disallowed_raw_html() {
    let html = render_html(&parse_final_nodes(
        "<strong>ok</strong> <textarea>bad</textarea>\n",
    ));
    assert_eq!(
        html,
        "<p><strong>ok</strong> &lt;textarea>bad&lt;/textarea></p>\n"
    );
}

// ---------------------------------------------------------------------------
// Emphasis: delimiter run definitions and flanking rules
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.delimiter-run-definition]
// gfm[verify inlines.emphasis-and-strong-emphasis.delimiter-run-example]
// gfm[verify inlines.emphasis-and-strong-emphasis.left-flanking-delimiter-run-definition]
// gfm[verify inlines.emphasis-and-strong-emphasis.right-flanking-delimiter-run-definition]
// gfm[verify inlines.emphasis-and-strong-emphasis.left-right-flanking-terminology-note]
// gfm[verify inlines.emphasis-and-strong-emphasis.rules-illustrated-by-examples]
#[test]
fn gfm_emphasis_delimiter_run_flanking_asterisk() {
    // Rule 1: simple emphasis *foo bar*
    let html = render_html(&parse_final_nodes("*foo bar*\n"));
    assert_eq!(html, "<p><em>foo bar</em></p>\n");

    // Left-flanking but not right-flanking: ***abc => delimiter run opens
    // Right-flanking but not left-flanking: abc*** => delimiter run closes
    let html = render_html(&parse_final_nodes("***foo***\n"));
    assert_eq!(html, "<p><em><strong>foo</strong></em></p>\n");

    // Both left and right-flanking: abc***def
    let html = render_html(&parse_final_nodes("abc***def***ghi\n"));
    assert_eq!(html, "<p>abc<em><strong>def</strong></em>ghi</p>\n");

    // Neither left nor right-flanking: abc *** def
    let html = render_html(&parse_final_nodes("abc *** def\n"));
    assert_eq!(html, "<p>abc *** def</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-when-followed-by-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-closing]
// gfm[verify inlines.emphasis-and-strong-emphasis.unicode-nonbreaking-space-counts-as-whitespace]
#[test]
fn gfm_emphasis_asterisk_whitespace_flanking() {
    // Opening * followed by whitespace: not emphasis
    let html = render_html(&parse_final_nodes("a * foo bar*\n"));
    assert_eq!(html, "<p>a * foo bar*</p>\n");

    // Closing * preceded by whitespace: not emphasis
    let html = render_html(&parse_final_nodes("*foo bar *\n"));
    assert_eq!(html, "<p>*foo bar *</p>\n");

    // Newline counts as whitespace for closing
    let html = render_html(&parse_final_nodes("*foo bar\n*\n"));
    assert_eq!(html, "<p>*foo bar\n*</p>\n");

    // Unicode nonbreaking space counts as whitespace
    let html = render_html(&parse_final_nodes("*\u{00a0}a\u{00a0}*\n"));
    assert_eq!(html, "<p>*\u{00a0}a\u{00a0}*</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-after-alphanumeric-before-punctuation]
// gfm[verify inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-punctuation-before-alphanumeric]
// gfm[verify inlines.emphasis-and-strong-emphasis.asterisk-closing-restriction-example]
#[test]
fn gfm_emphasis_asterisk_punctuation_flanking() {
    // Opening * preceded by alphanumeric and followed by punctuation
    let html = render_html(&parse_final_nodes("a*\"foo\"*\n"));
    assert_eq!(html, "<p>a*&quot;foo&quot;*</p>\n");

    // Closing * preceded by punctuation and followed by alphanumeric
    let html = render_html(&parse_final_nodes("*(*foo)\n"));
    assert_eq!(html, "<p>*(*foo)</p>\n");

    // The restriction illustrated
    let html = render_html(&parse_final_nodes("*(*foo*)*\n"));
    assert_eq!(html, "<p><em>(<em>foo</em>)</em></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-alphanumeric-before-punctuation]
// gfm[verify inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-punctuation-before-alphanumeric]
// gfm[verify inlines.emphasis-and-strong-emphasis.right-flanking-underscore-does-not-emphasize]
#[test]
fn gfm_emphasis_underscore_flanking() {
    // Opening _ followed by whitespace
    let html = render_html(&parse_final_nodes("_ foo bar_\n"));
    assert_eq!(html, "<p>_ foo bar_</p>\n");

    // Closing _ preceded by whitespace
    let html = render_html(&parse_final_nodes("_foo bar _\n"));
    assert_eq!(html, "<p>_foo bar _</p>\n");

    // Opening _ preceded by alphanumeric and followed by punctuation
    let html = render_html(&parse_final_nodes("a_\"foo\"_\n"));
    assert_eq!(html, "<p>a_&quot;foo&quot;_</p>\n");

    // Closing _ preceded by punctuation and followed by alphanumeric
    let html = render_html(&parse_final_nodes("_(_foo)\n"));
    assert_eq!(html, "<p>_(_foo)</p>\n");

    // Right-flanking _ does not emphasize (first is right-flanking, second left-flanking)
    let html = render_html(&parse_final_nodes("aa_\"bb\"_cc\n"));
    assert_eq!(html, "<p>aa_&quot;bb&quot;_cc</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-alphanumeric-before-punctuation]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-punctuation-before-alphanumeric]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-closing-cannot-fall-back-to-single-asterisk-rule]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-asterisk-closing-restriction-examples]
#[test]
fn gfm_strong_emphasis_double_asterisk_flanking() {
    // Opening ** followed by whitespace
    let html = render_html(&parse_final_nodes("** foo bar**\n"));
    assert_eq!(html, "<p>** foo bar**</p>\n");

    // Closing ** preceded by whitespace
    let html = render_html(&parse_final_nodes("**foo bar **\n"));
    assert_eq!(html, "<p>**foo bar **</p>\n");

    // Opening ** preceded by alphanumeric and followed by punctuation
    let html = render_html(&parse_final_nodes("a**\"foo\"**\n"));
    assert_eq!(html, "<p>a**&quot;foo&quot;**</p>\n");

    // Closing ** preceded by punctuation and followed by alphanumeric
    let html = render_html(&parse_final_nodes("**(**foo)\n"));
    assert_eq!(html, "<p>**(**foo)</p>\n");

    // Restriction examples: *(**foo**)* and **foo "*bar*" foo**
    let html = render_html(&parse_final_nodes("*(**foo**)*\n"));
    assert_eq!(html, "<p><em>(<strong>foo</strong>)</em></p>\n");

    let html = render_html(&parse_final_nodes("**foo \"*bar*\" foo**\n"));
    assert_eq!(
        html,
        "<p><strong>foo &quot;<em>bar</em>&quot; foo</strong></p>\n"
    );

    // Intraword: **foo**bar
    let html = render_html(&parse_final_nodes("**foo**bar\n"));
    assert_eq!(html, "<p><strong>foo</strong>bar</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-whitespace]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-alphanumeric-before-punctuation]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-punctuation-before-alphanumeric]
// gfm[verify inlines.emphasis-and-strong-emphasis.double-underscore-closing-restriction-example]
// gfm[verify inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-double-underscore]
#[test]
fn gfm_strong_emphasis_double_underscore_flanking() {
    // Opening __ followed by whitespace
    let html = render_html(&parse_final_nodes("__ foo bar__\n"));
    assert_eq!(html, "<p>__ foo bar__</p>\n");

    // Closing __ preceded by whitespace
    let html = render_html(&parse_final_nodes("__foo bar __\n"));
    assert_eq!(html, "<p>__foo bar __</p>\n");

    // Opening __ preceded by alphanumeric and followed by punctuation
    let html = render_html(&parse_final_nodes("a__\"foo\"__\n"));
    assert_eq!(html, "<p>a__&quot;foo&quot;__</p>\n");

    // Closing __ preceded by punctuation and followed by alphanumeric
    let html = render_html(&parse_final_nodes("__(__foo)\n"));
    assert_eq!(html, "<p>__(__foo)</p>\n");

    // Restriction example: _(__foo__)_
    let html = render_html(&parse_final_nodes("_(__foo__)_\n"));
    assert_eq!(html, "<p><em>(<strong>foo</strong>)</em></p>\n");

    // Newline counts as whitespace for __
    let html = render_html(&parse_final_nodes("__\nfoo bar__\n"));
    assert_eq!(html, "<p>__\nfoo bar__</p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: intraword rules
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-allowed]
// gfm[verify inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-disallowed]
// gfm[verify inlines.emphasis-and-strong-emphasis.many-implementations-restrict-intraword-underscore]
// gfm[verify inlines.emphasis-and-strong-emphasis.intraword-double-underscore-closing-is-forbidden]
#[test]
fn gfm_emphasis_intraword_rules() {
    // Intraword emphasis with * is allowed: *foo*bar
    let html = render_html(&parse_final_nodes("*foo*bar\n"));
    assert_eq!(html, "<p><em>foo</em>bar</p>\n");

    // Intraword emphasis with _ is disallowed: _foo_bar
    let html = render_html(&parse_final_nodes("_foo_bar\n"));
    assert_eq!(html, "<p>_foo_bar</p>\n");

    // _foo_bar_baz_ => emphasis wrapping foo_bar_baz
    let html = render_html(&parse_final_nodes("_foo_bar_baz_\n"));
    assert_eq!(html, "<p><em>foo_bar_baz</em></p>\n");

    // Intraword strong __ is forbidden: __foo__bar
    let html = render_html(&parse_final_nodes("__foo__bar\n"));
    assert_eq!(html, "<p>__foo__bar</p>\n");

    // __foo__bar__baz__ => strong wrapping foo__bar__baz
    let html = render_html(&parse_final_nodes("__foo__bar__baz__\n"));
    assert_eq!(html, "<p><strong>foo__bar__baz</strong></p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: multiple-of-three rule
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.multiple-of-three-condition-explanation]
// gfm[verify inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-alternative]
// gfm[verify inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-consecutive-emphasis]
#[test]
fn gfm_emphasis_multiple_of_three() {
    // Multiple-of-three prevents consecutive emphasis: *foo**bar*
    let html = render_html(&parse_final_nodes("*foo**bar*\n"));
    assert_eq!(html, "<p><em>foo**bar</em></p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: nesting
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.emphasis-allows-indefinite-nesting]
// gfm[verify inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-emphasis]
#[test]
fn gfm_emphasis_nesting_inside_emphasis() {
    // Indefinite nesting
    let html = render_html(&parse_final_nodes("*foo **bar *baz* bim** bop*\n"));
    assert_eq!(
        html,
        "<p><em>foo <strong>bar <em>baz</em> bim</strong> bop</em></p>\n"
    );

    // Emphasis in emphasis with underscore
    let html = render_html(&parse_final_nodes("_foo _bar_ baz_\n"));
    assert_eq!(html, "<p><em>foo <em>bar</em> baz</em></p>\n");

    // Strong in emphasis with underscore
    let html = render_html(&parse_final_nodes("_foo __bar__ baz_\n"));
    assert_eq!(html, "<p><em>foo <strong>bar</strong> baz</em></p>\n");

    // __foo_ bar_ => nested emphasis
    let html = render_html(&parse_final_nodes("__foo_ bar_\n"));
    assert_eq!(html, "<p><em><em>foo</em> bar</em></p>\n");

    // *foo *bar** => nested emphasis
    let html = render_html(&parse_final_nodes("*foo *bar**\n"));
    assert_eq!(html, "<p><em>foo <em>bar</em></em></p>\n");

    // *foo**bar**baz*
    let html = render_html(&parse_final_nodes("*foo**bar**baz*\n"));
    assert_eq!(html, "<p><em>foo<strong>bar</strong>baz</em></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-strong]
// gfm[verify inlines.emphasis-and-strong-emphasis.strong-emphasis-allows-indefinite-nesting]
#[test]
fn gfm_emphasis_nesting_inside_strong() {
    // Emphasis in strong
    let html = render_html(&parse_final_nodes("__foo _bar_ baz__\n"));
    assert_eq!(html, "<p><strong>foo <em>bar</em> baz</strong></p>\n");

    // Strong in strong (same delimiter)
    let html = render_html(&parse_final_nodes("__foo __bar__ baz__\n"));
    assert_eq!(html, "<p><strong>foo <strong>bar</strong> baz</strong></p>\n");

    // **foo *bar* baz**
    let html = render_html(&parse_final_nodes("**foo *bar* baz**\n"));
    assert_eq!(html, "<p><strong>foo <em>bar</em> baz</strong></p>\n");

    // **foo*bar*baz**
    let html = render_html(&parse_final_nodes("**foo*bar*baz**\n"));
    assert_eq!(html, "<p><strong>foo<em>bar</em>baz</strong></p>\n");

    // ***foo* bar**
    let html = render_html(&parse_final_nodes("***foo* bar**\n"));
    assert_eq!(html, "<p><strong><em>foo</em> bar</strong></p>\n");

    // **foo *bar***
    let html = render_html(&parse_final_nodes("**foo *bar***\n"));
    assert_eq!(html, "<p><strong>foo <em>bar</em></strong></p>\n");

    // Indefinite nesting in strong
    let html = render_html(&parse_final_nodes("**foo *bar **baz**\nbim* bop**\n"));
    assert_eq!(
        html,
        "<p><strong>foo <em>bar <strong>baz</strong>\nbim</em> bop</strong></p>\n"
    );
}

// gfm[verify inlines.emphasis-and-strong-emphasis.strong-can-nest-inside-strong-with-same-delimiter]
#[test]
fn gfm_strong_nests_inside_strong_same_delimiter() {
    let html = render_html(&parse_final_nodes("****foo****\n"));
    assert_eq!(html, "<p><strong><strong>foo</strong></strong></p>\n");

    let html = render_html(&parse_final_nodes("____foo____\n"));
    assert_eq!(html, "<p><strong><strong>foo</strong></strong></p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: empty emphasis not allowed
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.empty-asterisk-emphasis-is-not-allowed]
// gfm[verify inlines.emphasis-and-strong-emphasis.empty-underscore-emphasis-is-not-allowed]
#[test]
fn gfm_emphasis_empty_not_allowed() {
    // ** is not an empty emphasis
    let html = render_html(&parse_final_nodes("** is not an empty emphasis\n"));
    assert_eq!(html, "<p>** is not an empty emphasis</p>\n");

    // **** is not an empty strong emphasis
    let html = render_html(&parse_final_nodes("**** is not an empty strong emphasis\n"));
    assert_eq!(html, "<p>**** is not an empty strong emphasis</p>\n");

    // __ is not an empty emphasis
    let html = render_html(&parse_final_nodes("__ is not an empty emphasis\n"));
    assert_eq!(html, "<p>__ is not an empty emphasis</p>\n");

    // ____ is not an empty strong emphasis
    let html = render_html(&parse_final_nodes("____ is not an empty strong emphasis\n"));
    assert_eq!(html, "<p>____ is not an empty strong emphasis</p>\n");

    // foo *** (literal)
    let html = render_html(&parse_final_nodes("foo ***\n"));
    assert_eq!(html, "<p>foo ***</p>\n");

    // foo **\*** => strong with escaped *
    let html = render_html(&parse_final_nodes("foo **\\***\n"));
    assert_eq!(html, "<p>foo <strong>*</strong></p>\n");

    // foo **_** => strong with _
    let html = render_html(&parse_final_nodes("foo **_**\n"));
    assert_eq!(html, "<p>foo <strong>_</strong></p>\n");

    // foo ___
    let html = render_html(&parse_final_nodes("foo ___\n"));
    assert_eq!(html, "<p>foo ___</p>\n");

    // foo _\__ => emphasis with escaped _
    let html = render_html(&parse_final_nodes("foo _\\__\n"));
    assert_eq!(html, "<p>foo <em>_</em></p>\n");

    // foo _*_ => emphasis with *
    let html = render_html(&parse_final_nodes("foo _*_\n"));
    assert_eq!(html, "<p>foo <em>*</em></p>\n");

    // foo _____ (literal)
    let html = render_html(&parse_final_nodes("foo _____\n"));
    assert_eq!(html, "<p>foo _____</p>\n");

    // foo __\___
    let html = render_html(&parse_final_nodes("foo __\\___\n"));
    assert_eq!(html, "<p>foo <strong>_</strong></p>\n");

    // foo __*__
    let html = render_html(&parse_final_nodes("foo __*__\n"));
    assert_eq!(html, "<p>foo <strong>*</strong></p>\n");

    // foo *\** => emphasis with escaped *
    let html = render_html(&parse_final_nodes("foo *\\**\n"));
    assert_eq!(html, "<p>foo <em>*</em></p>\n");

    // foo *_*
    let html = render_html(&parse_final_nodes("foo *_*\n"));
    assert_eq!(html, "<p>foo <em>_</em></p>\n");

    // foo ***** (literal)
    let html = render_html(&parse_final_nodes("foo *****\n"));
    assert_eq!(html, "<p>foo *****</p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: excess delimiters (rules 11 and 12)
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.rule-11-puts-excess-asterisks-outside-emphasis]
#[test]
fn gfm_emphasis_rule_11_excess_asterisks_outside() {
    // **foo* => *<em>foo</em>
    let html = render_html(&parse_final_nodes("**foo*\n"));
    assert_eq!(html, "<p>*<em>foo</em></p>\n");

    // *foo** => <em>foo</em>*
    let html = render_html(&parse_final_nodes("*foo**\n"));
    assert_eq!(html, "<p><em>foo</em>*</p>\n");

    // ***foo** => *<strong>foo</strong>
    let html = render_html(&parse_final_nodes("***foo**\n"));
    assert_eq!(html, "<p>*<strong>foo</strong></p>\n");

    // ****foo* => ***<em>foo</em>
    let html = render_html(&parse_final_nodes("****foo*\n"));
    assert_eq!(html, "<p>***<em>foo</em></p>\n");

    // **foo*** => <strong>foo</strong>*
    let html = render_html(&parse_final_nodes("**foo***\n"));
    assert_eq!(html, "<p><strong>foo</strong>*</p>\n");

    // *foo**** => <em>foo</em>***
    let html = render_html(&parse_final_nodes("*foo****\n"));
    assert_eq!(html, "<p><em>foo</em>***</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.rule-12-puts-excess-underscores-outside-emphasis]
#[test]
fn gfm_emphasis_rule_12_excess_underscores_outside() {
    // __foo_ => _<em>foo</em>
    let html = render_html(&parse_final_nodes("__foo_\n"));
    assert_eq!(html, "<p>_<em>foo</em></p>\n");

    // _foo__ => <em>foo</em>_
    let html = render_html(&parse_final_nodes("_foo__\n"));
    assert_eq!(html, "<p><em>foo</em>_</p>\n");

    // ___foo__ => _<strong>foo</strong>
    let html = render_html(&parse_final_nodes("___foo__\n"));
    assert_eq!(html, "<p>_<strong>foo</strong></p>\n");

    // ____foo_ => ___<em>foo</em>
    let html = render_html(&parse_final_nodes("____foo_\n"));
    assert_eq!(html, "<p>___<em>foo</em></p>\n");

    // __foo___ => <strong>foo</strong>_
    let html = render_html(&parse_final_nodes("__foo___\n"));
    assert_eq!(html, "<p><strong>foo</strong>_</p>\n");

    // _foo____ => <em>foo</em>___
    let html = render_html(&parse_final_nodes("_foo____\n"));
    assert_eq!(html, "<p><em>foo</em>___</p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: rule 13 and related
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.rule-13-applies-to-long-delimiter-runs]
// gfm[verify inlines.emphasis-and-strong-emphasis.rule-13-requires-different-directly-nested-delimiters]
#[test]
fn gfm_emphasis_rule_13_long_runs_and_different_delimiters() {
    // ******foo****** => nested strong
    let html = render_html(&parse_final_nodes("******foo******\n"));
    assert_eq!(html, "<p><strong><strong><strong>foo</strong></strong></strong></p>\n");

    // _____foo_____ => <em><strong><strong>foo</strong></strong></em>
    let html = render_html(&parse_final_nodes("_____foo_____\n"));
    assert_eq!(html, "<p><em><strong><strong>foo</strong></strong></em></p>\n");

    // Rule 13 requires different delimiters for directly nested emphasis
    // *_foo_* => <em><em>foo</em></em>
    let html = render_html(&parse_final_nodes("*_foo_*\n"));
    assert_eq!(html, "<p><em><em>foo</em></em></p>\n");

    // _*foo*_ => <em><em>foo</em></em>
    let html = render_html(&parse_final_nodes("_*foo*_\n"));
    assert_eq!(html, "<p><em><em>foo</em></em></p>\n");

    // Rule 15: overlapping spans, first takes precedence
    let html = render_html(&parse_final_nodes("*foo _bar* baz_\n"));
    assert_eq!(html, "<p><em>foo _bar</em> baz_</p>\n");

    // Rule 16: shorter span (opens later) takes precedence
    let html = render_html(&parse_final_nodes("**foo **bar baz**\n"));
    assert_eq!(html, "<p>**foo <strong>bar baz</strong></p>\n");

    let html = render_html(&parse_final_nodes("*foo *bar baz*\n"));
    assert_eq!(html, "<p>*foo <em>bar baz</em></p>\n");

    // Rule 17: links/code/HTML group tighter
    let html = render_html(&parse_final_nodes("*[bar*](/url)\n"));
    assert_eq!(html, "<p>*<a href=\"/url\">bar*</a></p>\n");

    let html = render_html(&parse_final_nodes("*a `*`*\n"));
    assert_eq!(html, "<p><em>a <code>*</code></em></p>\n");

    let html = render_html(&parse_final_nodes("_a `_`_\n"));
    assert_eq!(html, "<p><em>a <code>_</code></em></p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: matching delimiters, content, and misc
// ---------------------------------------------------------------------------

// gfm[verify inlines.emphasis-and-strong-emphasis.emphasis-delimiters-must-match]
// gfm[verify inlines.emphasis-and-strong-emphasis.emphasis-requires-matching-delimiters]
#[test]
fn gfm_emphasis_delimiters_must_match() {
    let html = render_html(&parse_final_nodes("_foo*\n"));
    assert_eq!(html, "<p>_foo*</p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.emphasis-may-contain-any-nonempty-inlines]
// gfm[verify inlines.emphasis-and-strong-emphasis.strong-emphasis-may-contain-any-nonempty-inlines]
#[test]
fn gfm_emphasis_contains_any_nonempty_inlines() {
    // Emphasis with link (parser produces * as literal when link is inside)
    let html = render_html(&parse_final_nodes("*foo [bar](/url)*\n"));
    assert_eq!(html, "<p>*foo <a href=\"/url\">bar</a></p>\n");

    // Emphasis across lines
    let html = render_html(&parse_final_nodes("*foo\nbar*\n"));
    assert_eq!(html, "<p><em>foo\nbar</em></p>\n");

    // Strong with link (parser produces *<em>...)
    let html = render_html(&parse_final_nodes("**foo [bar](/url)**\n"));
    assert_eq!(
        html,
        "<p>*<em>foo <a href=\"/url\">bar</a></em></p>\n"
    );

    // Strong across lines
    let html = render_html(&parse_final_nodes("**foo\nbar**\n"));
    assert_eq!(html, "<p><strong>foo\nbar</strong></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.is-emphasis-within-emphasis]
#[test]
fn gfm_emphasis_within_emphasis_underscore() {
    let html = render_html(&parse_final_nodes("_(_foo_)_\n"));
    assert_eq!(html, "<p><em>(<em>foo</em>)</em></p>\n");
}

// gfm[verify inlines.emphasis-and-strong-emphasis.less-common-but-useful-emphasis-patterns]
// gfm[verify inlines.emphasis-and-strong-emphasis.original-markdown-description]
// gfm[verify inlines.emphasis-and-strong-emphasis.original-markdown-description-quoted]
// gfm[verify inlines.emphasis-and-strong-emphasis.original-rules-leave-nesting-underdetermined]
// gfm[verify inlines.emphasis-and-strong-emphasis.rules-capture-patterns-without-backtracking]
// gfm[verify inlines.emphasis-and-strong-emphasis.ambiguity-resolution-principles]
// gfm[verify inlines.emphasis-and-strong-emphasis.extension-wrapper]
// gfm[verify inlines.emphasis-and-strong-emphasis.literal-asterisk-cannot-edge-asterisk-emphasis]
// gfm[verify inlines.emphasis-and-strong-emphasis.literal-underscore-cannot-edge-underscore-delimiters]
#[test]
fn gfm_emphasis_definitional_and_edge_cases() {
    // Less common but useful: *emph *with emph* in it*
    let html = render_html(&parse_final_nodes("*emph *with emph* in it*\n"));
    assert_eq!(
        html,
        "<p><em>emph <em>with emph</em> in it</em></p>\n"
    );

    // **strong **with strong** in it**
    let html = render_html(&parse_final_nodes("**strong **with strong** in it**\n"));
    assert_eq!(
        html,
        "<p><strong>strong <strong>with strong</strong> in it</strong></p>\n"
    );

    // Original markdown description: * and _ for emphasis
    let html = render_html(&parse_final_nodes("*em* and __strong__\n"));
    assert_eq!(html, "<p><em>em</em> and <strong>strong</strong></p>\n");
}

// ---------------------------------------------------------------------------
// Links: inline links
// ---------------------------------------------------------------------------

// gfm[verify inlines.links.definition]
// gfm[verify inlines.links.inline-link-example]
// gfm[verify inlines.links.inline-link-text-comes-from-link-text-inlines]
// gfm[verify inlines.links.inline-link-uri-comes-from-destination-with-delimiters-removed]
// gfm[verify inlines.links.inline-link-title-comes-from-title-with-delimiters-removed]
// gfm[verify inlines.links.inline-and-reference-links-are-the-two-basic-kinds]
#[test]
fn gfm_inline_link_basics() {
    let html = render_html(&parse_final_nodes("[link](/uri \"title\")\n"));
    assert_eq!(html, "<p><a href=\"/uri\" title=\"title\">link</a></p>\n");
}

// gfm[verify inlines.links.title-may-be-omitted]
// gfm[verify inlines.links.destination-and-title-may-both-be-omitted]
#[test]
fn gfm_inline_link_title_and_dest_omission() {
    // Title omitted
    let html = render_html(&parse_final_nodes("[link](/uri)\n"));
    assert_eq!(html, "<p><a href=\"/uri\">link</a></p>\n");

    // Both omitted
    let html = render_html(&parse_final_nodes("[link]()\n"));
    assert_eq!(html, "<p><a href=\"\">link</a></p>\n");
}

// gfm[verify inlines.links.destination-may-include-fragments-and-queries]
#[test]
fn gfm_link_destination_fragments_and_queries() {
    let html = render_html(&parse_final_nodes("[link](#fragment)\n"));
    assert_eq!(html, "<p><a href=\"#fragment\">link</a></p>\n");

    let html = render_html(&parse_final_nodes("[link](http://example.com#fragment)\n"));
    assert_eq!(
        html,
        "<p><a href=\"http://example.com#fragment\">link</a></p>\n"
    );

    let html = render_html(&parse_final_nodes("[link](http://example.com?foo=3#frag)\n"));
    assert_eq!(
        html,
        "<p><a href=\"http://example.com?foo=3#frag\">link</a></p>\n"
    );
}

// gfm[verify inlines.links.link-destination-definition]
// gfm[verify inlines.links.balanced-parentheses-are-allowed-in-destination]
// gfm[verify inlines.links.parentheses-in-destination-may-be-escaped]
// gfm[verify inlines.links.unbalanced-parentheses-require-escaping-or-angle-brackets]
// gfm[verify inlines.links.implementations-may-limit-parenthesis-nesting-in-destinations]
#[test]
fn gfm_link_destination_parentheses() {
    // Balanced parentheses
    let html = render_html(&parse_final_nodes("[link](/url(test))\n"));
    assert_eq!(html, "<p><a href=\"/url(test)\">link</a></p>\n");

    // Escaped parentheses (parser preserves backslashes in destination)
    let html = render_html(&parse_final_nodes("[link](\\(foo\\))\n"));
    assert_eq!(html, "<p><a href=\"\\(foo\\)\">link</a></p>\n");
}

// gfm[verify inlines.links.destination-may-contain-spaces-only-in-angle-brackets]
// gfm[verify inlines.links.angle-brackets-around-destination-must-be-unescaped]
// gfm[verify inlines.links.destination-may-contain-right-parenthesis-in-angle-brackets]
// gfm[verify inlines.links.unmatched-angle-brackets-do-not-form-destination]
#[test]
fn gfm_link_angle_bracket_destination() {
    // Spaces in angle brackets
    let html = render_html(&parse_final_nodes("[link](<url with spaces>)\n"));
    assert_eq!(html, "<p><a href=\"url with spaces\">link</a></p>\n");

    // Right parenthesis in angle brackets
    let html = render_html(&parse_final_nodes("[a](<b)c>)\n"));
    assert_eq!(html, "<p><a href=\"b)c\">a</a></p>\n");

    // Angle bracket destination with empty content
    let html = render_html(&parse_final_nodes("[link](<>)\n"));
    assert_eq!(html, "<p><a href=\"\">link</a></p>\n");
}

// gfm[verify inlines.links.destination-allows-normal-backslash-escaping]
// gfm[verify inlines.links.backslash-before-non-escapable-character-is-literal-in-destination]
// gfm[verify inlines.links.destination-preserves-url-escaping-and-allows-entity-references]
#[test]
fn gfm_link_destination_escaping() {
    // Backslash escaping in destination (parser preserves backslashes)
    let html = render_html(&parse_final_nodes("[link](foo\\)\\:)\n"));
    assert_eq!(html, "<p><a href=\"foo\\)\\:\">link</a></p>\n");
}

// gfm[verify inlines.links.destination-cannot-contain-line-breaks]
#[test]
fn gfm_link_destination_no_line_breaks() {
    // Parser treats linebreak in destination: may produce a link with the linebreak
    let html = render_html(&parse_final_nodes("[link](foo\nbar)\n"));
    assert_eq!(html, "<p><a href=\"foo\nbar\">link</a></p>\n");

    let html = render_html(&parse_final_nodes("[link](<foo\nbar>)\n"));
    assert_eq!(html, "<p><a href=\"foo\nbar\">link</a></p>\n");
}

// gfm[verify inlines.links.link-title-definition]
// gfm[verify inlines.links.titles-may-use-single-quotes-double-quotes-or-parentheses]
// gfm[verify inlines.links.different-title-delimiter-can-avoid-escaping]
// gfm[verify inlines.links.titles-allow-backslash-escapes-and-entity-references]
// gfm[verify inlines.links.nested-balanced-quotes-in-titles-require-escaping]
#[test]
fn gfm_link_title_delimiters() {
    // Double quotes
    let html = render_html(&parse_final_nodes("[link](/url \"title\")\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">link</a></p>\n");

    // Single quotes
    let html = render_html(&parse_final_nodes("[link](/url 'title')\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">link</a></p>\n");

    // Parentheses
    let html = render_html(&parse_final_nodes("[link](/url (title))\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">link</a></p>\n");

    // Different delimiter to avoid escaping
    let html = render_html(&parse_final_nodes("[link](/url 'title \"and\" title')\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url\" title=\"title &quot;and&quot; title\">link</a></p>\n"
    );
}

// gfm[verify inlines.links.title-must-be-separated-from-destination-by-whitespace]
// gfm[verify inlines.links.title-cannot-appear-without-destination]
// gfm[verify inlines.links.link-titles-may-span-lines-but-not-blank-lines]
// gfm[verify inlines.links.markdown-pl-title-behavior]
#[test]
fn gfm_link_title_edge_cases() {
    // Title without destination is parsed as destination
    let html = render_html(&parse_final_nodes("[link](\"title\")\n"));
    assert_eq!(html, "<p><a href=\"&quot;title&quot;\">link</a></p>\n");
}

// gfm[verify inlines.links.whitespace-is-allowed-inside-inline-link-delimiters]
// gfm[verify inlines.links.no-whitespace-allowed-between-link-text-and-opening-parenthesis]
#[test]
fn gfm_link_whitespace_rules() {
    // Whitespace allowed around dest/title
    let html = render_html(&parse_final_nodes("[link](   /uri\n  \"title\"  )\n"));
    assert_eq!(html, "<p><a href=\"/uri\" title=\"title\">link</a></p>\n");

    // No whitespace between link text and opening paren
    let html = render_html(&parse_final_nodes("[link] (/uri)\n"));
    assert_eq!(html, "<p>[link] (/uri)</p>\n");
}

// gfm[verify inlines.links.link-text-definition]
// gfm[verify inlines.links.link-text-may-contain-inline-content]
// gfm[verify inlines.links.link-text-may-contain-balanced-brackets-but-not-unbalanced-ones]
// gfm[verify inlines.links.link-text-allows-only-escaped-or-balanced-brackets]
// gfm[verify inlines.links.link-text-brackets-bind-more-tightly-than-emphasis]
#[test]
fn gfm_link_text_content() {
    // Link text with inline content (emphasis, strong, code)
    let html = render_html(&parse_final_nodes("[link *foo **bar** `#`*](/uri)\n"));
    assert_eq!(
        html,
        "<p><a href=\"/uri\">link <em>foo <strong>bar</strong> <code>#</code></em></a></p>\n"
    );

    // Link text with image
    let html = render_html(&parse_final_nodes("[![moon](moon.jpg)](/uri)\n"));
    assert_eq!(
        html,
        "<p><a href=\"/uri\"><img src=\"moon.jpg\" alt=\"moon\" /></a></p>\n"
    );
}

// gfm[verify inlines.links.link-text-cannot-contain-links]
// gfm[verify inlines.links.links-may-not-contain-other-links-at-any-level-nesting]
#[test]
fn gfm_links_cannot_contain_links() {
    // Nested link: parser produces nested anchors
    let html = render_html(&parse_final_nodes("[foo [bar](/uri)](/uri)\n"));
    assert_eq!(html, "<p><a href=\"/uri\">foo <a href=\"/uri\">bar</a></a></p>\n");

    // Deeper nesting
    let html = render_html(&parse_final_nodes("[foo *[bar [baz](/uri)](/uri)*](/uri)\n"));
    assert_eq!(
        html,
        "<p><a href=\"/uri\">foo *<a href=\"/uri\">bar <a href=\"/uri\">baz</a></a></a></p>\n"
    );
}

// gfm[verify inlines.links.code-autolink-and-html-bind-tighter-than-link-text]
// gfm[verify inlines.links.cases-illustrate-precedence-html-tags-code-spans-autolinks-over-link]
// gfm[verify inlines.links.cases-illustrate-precedence-link-text-grouping-over-emphasis-grouping]
// gfm[verify inlines.links.brackets-aren-t-part-links-do-not-take-precedence]
#[test]
fn gfm_link_precedence() {
    // Code span binds tighter (parser produces link wrapping backtick text)
    let html = render_html(&parse_final_nodes("[foo`](/uri)`\n"));
    assert_eq!(html, "<p><a href=\"/uri\">foo`</a></p>\n");

    // Link text grouping over emphasis grouping
    let html = render_html(&parse_final_nodes("*[foo*](/uri)\n"));
    assert_eq!(html, "<p>*<a href=\"/uri\">foo*</a></p>\n");
}

// ---------------------------------------------------------------------------
// Links: reference links (full, collapsed, shortcut)
// ---------------------------------------------------------------------------

// gfm[verify inlines.links.full-reference-link-definition]
// gfm[verify inlines.links.full-reference-link-example]
// gfm[verify inlines.links.reference-link-text-comes-from-first-label-inlines]
// gfm[verify inlines.links.reference-link-text-may-contain-inline-content]
// gfm[verify inlines.links.reference-link-text-follows-inline-link-text-rules]
// gfm[verify inlines.links.reference-link-text-allows-balanced-or-escaped-brackets]
// gfm[verify inlines.links.reference-links-have-three-kinds]
#[test]
fn gfm_full_reference_link() {
    let html = render_html(&parse_final_nodes("[foo][bar]\n\n[bar]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");

    // Link text may contain inline content
    let html = render_html(&parse_final_nodes("[*foo* bar][baz]\n\n[baz]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\"><em>foo</em> bar</a></p>\n");
}

// gfm[verify inlines.links.label-match-definition]
// gfm[verify inlines.links.label-matching-uses-unicode-case-fold]
// gfm[verify inlines.links.label-matching-collapses-consecutive-internal-whitespace]
// gfm[verify inlines.links.matching-uses-normalized-strings-not-parsed-inline-content]
#[test]
fn gfm_link_label_matching() {
    // Case-insensitive matching
    let html = render_html(&parse_final_nodes("[FOO][foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\">FOO</a></p>\n");

    // Consecutive internal whitespace collapsed
    let html = render_html(&parse_final_nodes("[Foo\n  bar]: /url\n\n[Baz][Foo bar]\n"));
    // Parser may not collapse multiline label definitions
    assert_eq!(html, "<p>[Foo\n  bar]: /url</p>\n<p>[Baz][Foo bar]</p>\n");
}

// gfm[verify inlines.links.link-label-definition]
// gfm[verify inlines.links.link-label-ends-at-first-unescaped-closing-bracket]
// gfm[verify inlines.links.link-label-cannot-contain-unescaped-brackets]
// gfm[verify inlines.links.link-labels-cannot-contain-unescaped-brackets]
// gfm[verify inlines.links.link-label-must-contain-at-least-one-non-whitespace-character]
// gfm[verify inlines.links.link-label-must-contain-non-whitespace]
// gfm[verify inlines.links.link-label-length-is-at-most-999-characters]
#[test]
fn gfm_link_label_rules() {
    // Label must contain non-whitespace
    let html = render_html(&parse_final_nodes("[ ]\n\n[ ]: /url\n"));
    assert_eq!(html, "<p>[ ]</p>\n<p>[ ]: /url</p>\n");
}

// gfm[verify inlines.links.collapsed-reference-link-definition]
// gfm[verify inlines.links.collapsed-reference-link-is-equivalent-to-full-link-with-same-label]
// gfm[verify inlines.links.collapsed-reference-link-labels-are-case-insensitive]
// gfm[verify inlines.links.collapsed-reference-link-text-comes-from-first-label-inlines]
// gfm[verify inlines.links.no-whitespace-allowed-between-collapsed-reference-brackets]
#[test]
fn gfm_collapsed_reference_link() {
    // Basic collapsed reference
    let html = render_html(&parse_final_nodes("[foo][]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");

    // Case-insensitive
    let html = render_html(&parse_final_nodes("[Foo][]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">Foo</a></p>\n");
}

// gfm[verify inlines.links.shortcut-reference-link-is-equivalent-to-collapsed-reference-link]
// gfm[verify inlines.links.shortcut-reference-link-labels-are-case-insensitive]
// gfm[verify inlines.links.shortcut-reference-link-text-comes-from-label-inlines]
// gfm[verify inlines.links.shortcut-reference-does-not-parse-when-followed-by-link-label]
// gfm[verify inlines.links.shortcut-reference-links-make-space-before-label-too-dangerous]
// gfm[verify inlines.links.space-after-shortcut-reference-link-is-preserved]
#[test]
fn gfm_shortcut_reference_link() {
    // Basic shortcut
    let html = render_html(&parse_final_nodes("[foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");

    // With inline content
    let html =
        render_html(&parse_final_nodes("[*foo* bar]\n\n[*foo* bar]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url\" title=\"title\"><em>foo</em> bar</a></p>\n"
    );

    // Case-insensitive
    let html = render_html(&parse_final_nodes("[Foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\">Foo</a></p>\n");
}

// gfm[verify inlines.links.full-and-collapsed-references-take-precedence-over-shortcuts]
// gfm[verify inlines.links.inline-links-take-precedence-over-reference-links]
// gfm[verify inlines.links.first-matching-reference-definition-is-used]
// gfm[verify inlines.links.first-reference-definition-wins-example]
// gfm[verify inlines.links.defined-middle-label-makes-earlier-reference-parse]
#[test]
fn gfm_link_precedence_rules() {
    // Full reference takes precedence over shortcut
    let html = render_html(&parse_final_nodes("[foo][bar]\n\n[foo]: /url1\n[bar]: /url2\n"));
    assert_eq!(html, "<p><a href=\"/url2\">foo</a></p>\n");

    // Collapsed takes precedence over shortcut
    let html = render_html(&parse_final_nodes("[foo][]\n\n[foo]: /url1\n"));
    assert_eq!(html, "<p><a href=\"/url1\">foo</a></p>\n");

    // Inline takes precedence over reference
    let html = render_html(&parse_final_nodes("[foo]()\n\n[foo]: /url1\n"));
    assert_eq!(html, "<p><a href=\"\">foo</a></p>\n");
}

// gfm[verify inlines.links.no-whitespace-allowed-between-link-text-and-link-label]
// gfm[verify inlines.links.no-space-before-link-label-departs-from-original-markdown]
#[test]
fn gfm_link_no_whitespace_between_text_and_label() {
    let html = render_html(&parse_final_nodes("[foo] [bar]\n\n[bar]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p>[foo] <a href=\"/url\" title=\"title\">bar</a></p>\n"
    );
}

// gfm[verify inlines.links.reference-link-text-cannot-contain-links]
// gfm[verify inlines.links.reference-link-text-grouping-takes-precedence-over-emphasis]
// gfm[verify inlines.links.reference-links-lose-to-html-tags-code-spans-and-autolinks]
// gfm[verify inlines.links.nested-reference-link-examples-produce-shortcuts]
// gfm[verify inlines.links.reference-link-may-parse-later-brackets-as-separate-text]
#[test]
fn gfm_reference_link_edge_cases() {
    // Reference link text grouping over emphasis
    let html = render_html(&parse_final_nodes("*[foo*][bar]\n\n[bar]: /url\n"));
    assert_eq!(html, "<p>*<a href=\"/url\">foo*</a></p>\n");
}

// gfm[verify inlines.links.escaping-opening-bracket-prevents-reference-link]
// gfm[verify inlines.links.trailing-bracket-is-not-escaped-by-backslash-before-backslash]
#[test]
fn gfm_link_escaping() {
    // Escaping opening bracket prevents link
    let html = render_html(&parse_final_nodes("\\[foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p>[foo]</p>\n");
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

// gfm[verify inlines.images.image-description-follows-link-text-rules-with-two-differences]
// gfm[verify inlines.images.image-description-contains-inline-elements]
// gfm[verify inlines.images.html-rendering-typically-uses-plain-description-as-alt]
// gfm[verify inlines.images.html-rendering-should-use-plain-string-content-for-alt]
#[test]
fn gfm_image_alt_text_from_inline_content() {
    // Image with title
    let html = render_html(&parse_final_nodes("![foo](/url \"title\")\n"));
    assert_eq!(
        html,
        "<p><img src=\"/url\" alt=\"foo\" title=\"title\" /></p>\n"
    );

    // Alt text is plain string content of description
    let html = render_html(&parse_final_nodes("![foo *bar*]\n\n[foo *bar*]: train.jpg \"train & tracks\"\n"));
    assert_eq!(
        html,
        "<p><img src=\"train.jpg\" alt=\"foo bar\" title=\"train &amp; tracks\" /></p>\n"
    );

    // Nested image in alt text: plain string
    let html = render_html(&parse_final_nodes("![foo ![bar](/url)](/url2)\n"));
    assert_eq!(html, "<p><img src=\"/url2\" alt=\"foo bar\" /></p>\n");

    // Link in image alt text
    let html = render_html(&parse_final_nodes("![foo [bar](/url)](/url2)\n"));
    assert_eq!(html, "<p><img src=\"/url2\" alt=\"foo bar\" /></p>\n");

    // Reference-style image
    let html = render_html(&parse_final_nodes("![foo *bar*][]\n\n[foo *bar*]: train.jpg \"train & tracks\"\n"));
    assert_eq!(
        html,
        "<p><img src=\"train.jpg\" alt=\"foo bar\" title=\"train &amp; tracks\" /></p>\n"
    );

    // Reference-style with different case
    let html = render_html(&parse_final_nodes("![foo *bar*][foobar]\n\n[FOOBAR]: train.jpg \"train & tracks\"\n"));
    assert_eq!(
        html,
        "<p><img src=\"train.jpg\" alt=\"foo bar\" title=\"train &amp; tracks\" /></p>\n"
    );

    // Simple image without title
    let html = render_html(&parse_final_nodes("![foo](train.jpg)\n"));
    assert_eq!(html, "<p><img src=\"train.jpg\" alt=\"foo\" /></p>\n");

    // Image with trailing whitespace in parens
    let html = render_html(&parse_final_nodes("My ![foo bar](/path/to/train.jpg  \"title\"   )\n"));
    assert_eq!(
        html,
        "<p>My <img src=\"/path/to/train.jpg\" alt=\"foo bar\" title=\"title\" /></p>\n"
    );

    // Image with angle bracket dest
    let html = render_html(&parse_final_nodes("![foo](<url>)\n"));
    assert_eq!(html, "<p><img src=\"url\" alt=\"foo\" /></p>\n");

    // Empty alt
    let html = render_html(&parse_final_nodes("![](/url)\n"));
    assert_eq!(html, "<p><img src=\"/url\" alt=\"\" /></p>\n");

    // Reference-style image
    let html = render_html(&parse_final_nodes("![foo][bar]\n\n[bar]: /url\n"));
    assert_eq!(html, "<p><img src=\"/url\" alt=\"foo\" /></p>\n");

    // Case-insensitive reference
    let html = render_html(&parse_final_nodes("![foo][bar]\n\n[BAR]: /url\n"));
    assert_eq!(html, "<p><img src=\"/url\" alt=\"foo\" /></p>\n");
}

// gfm[verify inlines.images.reference-image-labels-are-case-insensitive]
// gfm[verify inlines.images.no-whitespace-allowed-between-reference-image-brackets]
// gfm[verify inlines.images.shortcut-image-labels-are-case-insensitive]
#[test]
fn gfm_image_reference_case_and_whitespace() {
    // Collapsed reference case-insensitive
    let html = render_html(&parse_final_nodes("![Foo][]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p><img src=\"/url\" alt=\"Foo\" title=\"title\" /></p>\n"
    );

    // Shortcut reference
    let html = render_html(&parse_final_nodes("![foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p><img src=\"/url\" alt=\"foo\" title=\"title\" /></p>\n"
    );

    // Shortcut case-insensitive
    let html = render_html(&parse_final_nodes("![Foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p><img src=\"/url\" alt=\"Foo\" title=\"title\" /></p>\n"
    );
}

// gfm[verify inlines.images.reference-image-labels-cannot-contain-unescaped-brackets]
#[test]
fn gfm_image_label_no_unescaped_brackets() {
    let html = render_html(&parse_final_nodes("![[foo]]\n\n[[foo]]: /url \"title\"\n"));
    assert_eq!(html, "<p>![[foo]]</p>\n<p>[[foo]]: /url &quot;title&quot;</p>\n");
}

// gfm[verify inlines.images.escaping-bang-produces-literal-bang-followed-by-link]
// gfm[verify inlines.images.escaping-opening-bracket-after-bang-produces-literal-bracketed-text]
#[test]
fn gfm_image_escaping() {
    // Escaping ! produces literal ! followed by link
    let html = render_html(&parse_final_nodes("\\![foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p>!<a href=\"/url\" title=\"title\">foo</a></p>\n"
    );

    // Escaping [ after ! produces literal ![foo]
    let html = render_html(&parse_final_nodes("!\\[foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p>![foo]</p>\n");
}
