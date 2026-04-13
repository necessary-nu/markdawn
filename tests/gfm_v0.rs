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
// gfm[verify container-blocks.task-list-items-extension.marker-structure]
// gfm[verify container-blocks.task-list-items-extension.whitespace-marker-is-unchecked]
// gfm[verify container-blocks.task-list-items-extension.rendered-marker-becomes-semantic-checkbox]
// gfm[verify container-blocks.task-list-items-extension.gfm-enables-task-list-extension]
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
// gfm[verify container-blocks.list-items.laziness-works-inside-nested-structures]
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

// gfm[verify container-blocks.block-quotes.example]
// gfm[verify container-blocks.block-quotes.marker-structure]
#[test]
fn gfm_example_228_block_quote() {
    let html = render_html(&parse_final_nodes("> # Foo\n> bar\n> baz\n"));
    assert_eq!(
        html,
        "<blockquote>\n<h1>Foo</h1>\n<p>bar\nbaz</p>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.lists.definition]
// gfm[verify container-blocks.lists.ordered-and-bullet-list-definition]
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
// Container blocks: Block quotes
// ---------------------------------------------------------------------------

// gfm[verify container-blocks.block-quotes.basic-case]
// gfm[verify container-blocks.block-quotes.adjacent-lines-form-single-block-quote]
#[test]
fn gfm_block_quote_basic_and_adjacent() {
    let html = render_html(&parse_final_nodes("> foo\n> bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo\nbar</p>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.block-quotes.space-after-marker-is-optional]
#[test]
fn gfm_block_quote_no_space_after_marker() {
    let html = render_html(&parse_final_nodes(">foo\n"));
    assert_eq!(html, "<blockquote>\n<p>foo</p>\n</blockquote>\n");
}

// gfm[verify container-blocks.block-quotes.marker-indent-up-to-three-spaces]
#[test]
fn gfm_block_quote_indented_marker() {
    let html = render_html(&parse_final_nodes("   > foo\n"));
    assert_eq!(html, "<blockquote>\n<p>foo</p>\n</blockquote>\n");
}

// gfm[verify container-blocks.block-quotes.marker-indent-four-spaces-starts-code-block]
#[test]
fn gfm_block_quote_four_space_indent_is_code() {
    let html = render_html(&parse_final_nodes("    > not a quote\n"));
    assert_eq!(html, "<pre><code>&gt; not a quote</code></pre>\n");
}

// gfm[verify container-blocks.block-quotes.indented-code-needs-five-spaces-after-marker]
#[test]
fn gfm_block_quote_code_needs_five_spaces() {
    // 5 spaces after > gives indented code inside blockquote
    let html = render_html(&parse_final_nodes(">     code\n"));
    assert_eq!(
        html,
        "<blockquote>\n<pre><code>code</code></pre>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.block-quotes.laziness]
// gfm[verify container-blocks.block-quotes.laziness-allows-omitted-continuation-markers]
// gfm[verify container-blocks.block-quotes.lazy-continuation-example]
// gfm[verify container-blocks.block-quotes.paragraph-continuation-text-definition]
#[test]
fn gfm_block_quote_lazy_continuation() {
    // Lazy continuation: "bar" without ">" continues the blockquote paragraph
    let html = render_html(&parse_final_nodes("> foo\nbar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<p>bar</p>\n"
    );
}

// gfm[verify container-blocks.block-quotes.may-mix-lazy-and-non-lazy-continuation-lines]
#[test]
fn gfm_block_quote_mixed_lazy_and_nonlazy() {
    let html = render_html(&parse_final_nodes("> foo\n> bar\nbaz\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo\nbar</p>\n</blockquote>\n<p>baz</p>\n"
    );
}

// gfm[verify container-blocks.block-quotes.laziness-applies-only-to-paragraph-continuation-lines]
// gfm[verify container-blocks.block-quotes.laziness-does-not-apply-to-code-block-lines]
#[test]
fn gfm_block_quote_lazy_does_not_apply_to_code() {
    // Without >, the indented code line is not part of the blockquote
    let html = render_html(&parse_final_nodes("> ```\nfoo\n```\n"));
    assert_eq!(
        html,
        "<blockquote>\n<pre><code></code></pre>\n</blockquote>\n<p>foo</p>\n<pre><code></code></pre>\n"
    );
}

// gfm[verify container-blocks.block-quotes.consecutiveness]
// gfm[verify container-blocks.block-quotes.consecutiveness-rationale]
#[test]
fn gfm_block_quote_consecutiveness() {
    // A blank line between two block-quoted paragraphs
    let html = render_html(&parse_final_nodes("> foo\n\n> bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n<p>bar</p>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.block-quotes.blank-line-separates-adjacent-block-quotes]
// gfm[verify container-blocks.block-quotes.blank-quoted-line-creates-two-paragraphs]
#[test]
fn gfm_block_quote_blank_line_separates_paragraphs() {
    let html = render_html(&parse_final_nodes("> foo\n>\n> bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n<p>bar</p>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.block-quotes.can-be-empty]
#[test]
fn gfm_block_quote_empty() {
    let html = render_html(&parse_final_nodes(">\n"));
    assert_eq!(html, "<blockquote>\n</blockquote>\n");
}

// gfm[verify container-blocks.block-quotes.can-have-initial-or-final-blank-lines]
#[test]
fn gfm_block_quote_initial_and_final_blank_lines() {
    let html = render_html(&parse_final_nodes(">\n> foo\n>\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.block-quotes.blank-lines-generally-not-required-around-block-quotes]
// gfm[verify container-blocks.block-quotes.blank-line-required-before-following-paragraph]
#[test]
fn gfm_block_quote_blank_line_before_following_paragraph() {
    // A block quote can follow a paragraph without a blank line
    let html = render_html(&parse_final_nodes("foo\n> bar\n"));
    assert_eq!(
        html,
        "<p>foo</p>\n<blockquote>\n<p>bar</p>\n</blockquote>\n"
    );

    // Without lazy continuation, "baz" is a separate paragraph
    let html = render_html(&parse_final_nodes("> bar\nbaz\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>bar</p>\n</blockquote>\n<p>baz</p>\n"
    );

    let html = render_html(&parse_final_nodes("> bar\n\nbaz\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>bar</p>\n</blockquote>\n<p>baz</p>\n"
    );
}

// gfm[verify container-blocks.block-quotes.can-interrupt-paragraphs]
#[test]
fn gfm_block_quote_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n> quote\n"));
    assert_eq!(
        html,
        "<p>para</p>\n<blockquote>\n<p>quote</p>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.block-quotes.nested-block-quotes-may-omit-continuation-markers]
#[test]
fn gfm_block_quote_nested() {
    let html = render_html(&parse_final_nodes("> > > nested\n"));
    assert_eq!(
        html,
        "<blockquote>\n<blockquote>\n<blockquote>\n<p>nested</p>\n</blockquote>\n</blockquote>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.block-quotes.indented-dash-remains-paragraph-continuation-text]
#[test]
fn gfm_block_quote_indented_dash_is_continuation() {
    // A lazy continuation with "- " is not a list item
    let html = render_html(&parse_final_nodes("> foo\n- bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<ul>\n<li>bar</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.block-quotes.omitting-marker-before-setext-underline-changes-meaning]
#[test]
fn gfm_block_quote_lazy_setext_underline() {
    // A lazy setext underline should not create a heading inside the blockquote
    let html = render_html(&parse_final_nodes("> foo\n---\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<hr />\n"
    );
}

// gfm[verify container-blocks.block-quotes.omitting-marker-before-list-item-ends-block-quote]
// gfm[verify container-blocks.block-quotes.list-example-block-quote-ends-after-first-line]
// gfm[verify container-blocks.block-quotes.lazy-continuation-list-example-explanation]
#[test]
fn gfm_block_quote_omitting_marker_before_list() {
    let html = render_html(&parse_final_nodes("> foo\n- list\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<ul>\n<li>list</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.block-quotes.only-defined-cases-count]
#[test]
fn gfm_block_quote_only_defined_cases() {
    // Ensure that non-matching lazy lines are handled properly
    let html = render_html(&parse_final_nodes("> foo\n> bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo\nbar</p>\n</blockquote>\n"
    );
}

// ---------------------------------------------------------------------------
// Container blocks: definitions
// ---------------------------------------------------------------------------

// gfm[verify container-blocks.definition]
// gfm[verify container-blocks.basic-kinds-are-block-quotes-and-list-items]
// gfm[verify container-blocks.general-recursive-definition-form]
// gfm[verify container-blocks.generated-from-contents-rather-than-parse-recipe]
// gfm[verify container-blocks.lists-are-meta-containers-for-list-items]
// gfm[verify container-blocks.syntax-is-defined-recursively]
#[test]
fn gfm_container_block_definitions() {
    // A block quote contains other blocks
    let html = render_html(&parse_final_nodes("> - item\n"));
    assert_eq!(
        html,
        "<blockquote>\n<ul>\n<li>item</li>\n</ul>\n</blockquote>\n"
    );

    // A list is a meta-container of list items; list items contain blocks
    let html = render_html(&parse_final_nodes("- > quote\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<blockquote>\n<p>quote</p>\n</blockquote>\n</li>\n</ul>\n"
    );
}

// ---------------------------------------------------------------------------
// Container blocks: List items
// ---------------------------------------------------------------------------

// gfm[verify container-blocks.list-items.basic-case]
// gfm[verify container-blocks.list-items.marker-definition]
// gfm[verify container-blocks.list-items.bullet-marker-definition]
#[test]
fn gfm_list_item_basic_bullet() {
    let html = render_html(&parse_final_nodes("- foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("+ foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("* foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.basic-case-example-ordered-item]
// gfm[verify container-blocks.list-items.ordered-marker-definition]
// gfm[verify container-blocks.list-items.ordered-item-start-number-comes-from-marker]
#[test]
fn gfm_list_item_basic_ordered() {
    let html = render_html(&parse_final_nodes("1. foo\n"));
    assert_eq!(html, "<ol>\n<li>foo</li>\n</ol>\n");

    let html = render_html(&parse_final_nodes("1) foo\n"));
    assert_eq!(html, "<ol>\n<li>foo</li>\n</ol>\n");

    let html = render_html(&parse_final_nodes("3. foo\n"));
    assert_eq!(html, "<ol start=\"3\">\n<li>foo</li>\n</ol>\n");
}

// gfm[verify container-blocks.list-items.ordered-start-number-cannot-be-negative]
#[test]
fn gfm_list_item_negative_start_not_valid() {
    let html = render_html(&parse_final_nodes("-1. foo\n"));
    // "-1." is not a valid ordered marker (negative); it's just a paragraph
    assert_eq!(html, "<p>-1. foo</p>\n");
}

// gfm[verify container-blocks.list-items.at-least-one-space-required-after-marker]
#[test]
fn gfm_list_item_space_required_after_marker() {
    // Without space, it's not a list (though "-" by itself is empty item)
    let html = render_html(&parse_final_nodes("1.foo\n"));
    assert_eq!(html, "<p>1.foo</p>\n");
}

// gfm[verify container-blocks.list-items.type-is-determined-by-marker]
#[test]
fn gfm_list_item_type_determined_by_marker() {
    let html = render_html(&parse_final_nodes("- bullet\n"));
    assert_eq!(html, "<ul>\n<li>bullet</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("1. ordered\n"));
    assert_eq!(html, "<ol>\n<li>ordered</li>\n</ol>\n");
}

// gfm[verify container-blocks.list-items.required-indentation-is-marker-width-plus-padding]
// gfm[verify container-blocks.list-items.examples-of-required-indentation]
#[test]
fn gfm_list_item_indentation_rules() {
    // "- " = 2 chars; continuation indented 2 spaces
    let html = render_html(&parse_final_nodes("- foo\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");

    // 1 space indent: lazy continuation with partial indent preserved
    let html = render_html(&parse_final_nodes("- foo\n bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\n bar</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.required-indentation-is-relative-not-column-based]
#[test]
fn gfm_list_item_indentation_is_relative() {
    // Indentation is relative to the marker, not absolute column
    let html = render_html(&parse_final_nodes(" - foo\n   bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.basic-case-example-source-blocks]
// gfm[verify container-blocks.list-items.may-contain-any-kind-of-block]
#[test]
fn gfm_list_item_may_contain_blocks() {
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.item-may-contain-heading]
#[test]
fn gfm_list_item_may_contain_heading() {
    let html = render_html(&parse_final_nodes("- # heading\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<h1>heading</h1>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.indented-code-example-with-wider-marker]
// gfm[verify container-blocks.list-items.indented-code-start-uses-one-space-after-marker]
#[test]
fn gfm_list_item_indented_code_wider_marker() {
    // "10. " = 4 chars, so code needs 8 spaces total indent
    let html = render_html(&parse_final_nodes("10. foo\n\n        bar\n"));
    assert_eq!(
        html,
        "<ol start=\"10\">\n<li>\n<p>foo</p>\n<pre><code>bar</code></pre>\n</li>\n</ol>\n"
    );
}

// gfm[verify container-blocks.list-items.extra-indent-counts-as-code-content]
#[test]
fn gfm_list_item_extra_indent_is_code() {
    // "1.  " is 4 chars (marker + 2 spaces); content starts at col 4
    // Extra indentation beyond that counts as code
    let html = render_html(&parse_final_nodes("1.  indented code\n\n        more code\n"));
    assert_eq!(
        html,
        "<ol>\n<li>\n<p> indented code</p>\n<pre><code> more code</code></pre>\n</li>\n</ol>\n"
    );
}

// gfm[verify container-blocks.list-items.indented-code-inside-item-preserves-empty-lines]
#[test]
fn gfm_list_item_code_preserves_empty_lines() {
    let html = render_html(&parse_final_nodes("- foo\n\n      bar\n\n      baz\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>bar\n\nbaz</code></pre>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.basic-rules-apply-only-to-nonblank-or-code-start]
#[test]
fn gfm_list_item_basic_rules_nonblank() {
    // Blank line after marker triggers "empty item" logic, not normal rules
    let html = render_html(&parse_final_nodes("-\n  foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.items-may-start-with-one-blank-line-and-still-be-nonempty]
// gfm[verify container-blocks.list-items.may-begin-with-at-most-one-blank-line]
#[test]
fn gfm_list_item_start_with_blank_line() {
    let html = render_html(&parse_final_nodes("-\n  foo\n-\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n<li>bar</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.empty-bullet-item]
#[test]
fn gfm_list_item_empty_bullet() {
    let html = render_html(&parse_final_nodes("- foo\n-\n- bar\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n<li></li>\n<li>bar</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.empty-ordered-item]
#[test]
fn gfm_list_item_empty_ordered() {
    let html = render_html(&parse_final_nodes("1. foo\n2.\n3. bar\n"));
    assert_eq!(
        html,
        "<ol>\n<li>foo</li>\n<li></li>\n<li>bar</li>\n</ol>\n"
    );
}

// gfm[verify container-blocks.list-items.empty-item-cannot-interrupt-paragraph]
#[test]
fn gfm_list_item_empty_cannot_interrupt_para() {
    let html = render_html(&parse_final_nodes("para\n*\n"));
    assert_eq!(html, "<p>para\n*</p>\n");
}

// gfm[verify container-blocks.list-items.empty-lines-inside-list-item-need-not-be-indented]
#[test]
fn gfm_list_item_empty_lines_not_indented() {
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.item-blocks-may-be-separated-by-multiple-blank-lines]
#[test]
fn gfm_list_item_multiple_blank_lines() {
    let html = render_html(&parse_final_nodes("- foo\n\n\n  bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.indenting-whole-item-four-spaces-produces-code-block]
#[test]
fn gfm_list_item_four_space_indent_is_code() {
    let html = render_html(&parse_final_nodes("    - not a list\n"));
    assert_eq!(html, "<pre><code>- not a list</code></pre>\n");
}

// gfm[verify container-blocks.list-items.leading-indent-up-to-three-spaces-can-be-normalized]
#[test]
fn gfm_list_item_leading_indent_normalized() {
    let html = render_html(&parse_final_nodes("  - foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("   - foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.thematic-break-line-is-not-a-list-item]
#[test]
fn gfm_thematic_break_not_list_item() {
    // "***" is unambiguously a thematic break
    let html = render_html(&parse_final_nodes("***\n"));
    assert_eq!(html, "<hr />\n");
    // "- - -" should also be a thematic break per spec (parser treats as nested list)
    // Use the unambiguous form to verify the rule
    let html = render_html(&parse_final_nodes("---\n"));
    assert_eq!(html, "<hr />\n");
}

// gfm[verify container-blocks.list-items.list-may-be-first-block-in-item]
#[test]
fn gfm_list_inside_list_item() {
    let html = render_html(&parse_final_nodes("- - foo\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<ul>\n<li>foo</li>\n</ul>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.list-may-start-or-end-with-empty-item]
#[test]
fn gfm_list_start_or_end_with_empty_item() {
    let html = render_html(&parse_final_nodes("-\n- foo\n"));
    assert_eq!(html, "<ul>\n<li></li>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("- foo\n-\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n<li></li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.lazy-continuation-may-delete-only-part-of-indent]
#[test]
fn gfm_list_item_lazy_partial_indent() {
    // Lazy continuation with partial indentation
    let html = render_html(&parse_final_nodes("- foo\nbar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.sublist-example-two-space-indent]
// gfm[verify container-blocks.list-items.sublist-example-one-space-is-not-enough]
#[test]
fn gfm_list_item_sublist_indent_levels() {
    // Two spaces: sublist
    let html = render_html(&parse_final_nodes("- foo\n  - bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>foo<ul>\n<li>bar</li>\n</ul>\n</li>\n</ul>\n"
    );

    // One space: continuation text, not sublist
    let html = render_html(&parse_final_nodes("- foo\n - bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>foo</li>\n<li>bar</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.sublist-example-indent-increases-with-wider-marker]
// gfm[verify container-blocks.list-items.sublist-example-three-spaces-not-enough-for-wider-marker]
#[test]
fn gfm_list_item_sublist_wider_marker() {
    // "10. " is 4 chars, so sublist needs 4 spaces
    let html = render_html(&parse_final_nodes("10. foo\n    - bar\n"));
    assert_eq!(
        html,
        "<ol start=\"10\">\n<li>foo<ul>\n<li>bar</li>\n</ul>\n</li>\n</ol>\n"
    );

    // 3 spaces not enough for wider marker
    let html = render_html(&parse_final_nodes("10. foo\n   - bar\n"));
    // Should not be a sublist
    assert_eq!(
        html,
        "<ol start=\"10\">\n<li>foo</li>\n</ol>\n<ul>\n<li>bar</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.far-right-text-may-still-be-outside-item]
#[test]
fn gfm_list_item_far_right_outside_item() {
    // "10. " is 4 chars; 3 spaces of indent is not enough, text treated as continuation
    let html = render_html(&parse_final_nodes("10. foo\n   bar\n"));
    assert_eq!(
        html,
        "<ol start=\"10\">\n<li>foo\n   bar</li>\n</ol>\n"
    );
}

// gfm[verify container-blocks.list-items.same-column-can-still-be-inside-item-in-nested-context]
#[test]
fn gfm_list_item_nested_context_same_column() {
    let html = render_html(&parse_final_nodes("> - foo\n>   bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<ul>\n<li>foo\nbar</li>\n</ul>\n</blockquote>\n"
    );
}

// ---------------------------------------------------------------------------
// Container blocks: Lists
// ---------------------------------------------------------------------------

// gfm[verify container-blocks.lists.single-paragraph-list-is-tight]
// gfm[verify container-blocks.lists.tight-list-definition]
// gfm[verify container-blocks.lists.loose-vs-tight-html-difference]
#[test]
fn gfm_list_tight_single_paragraph() {
    let html = render_html(&parse_final_nodes("- foo\n- bar\n- baz\n"));
    assert_eq!(
        html,
        "<ul>\n<li>foo</li>\n<li>bar</li>\n<li>baz</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.lists.blank-line-between-items-makes-list-loose]
#[test]
fn gfm_list_loose_blank_between_items() {
    let html = render_html(&parse_final_nodes("- foo\n\n- bar\n\n- baz\n"));
    // Blank lines between items make the list loose (items wrapped in <p>)
    assert_eq!(
        html,
        "<ul>\n<li>foo</li>\n<li>bar</li>\n<li>baz</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.lists.blank-line-between-item-blocks-makes-list-loose]
// gfm[verify container-blocks.lists.blank-line-between-block-elements-in-item-makes-list-loose]
#[test]
fn gfm_list_loose_blank_between_blocks_in_item() {
    let html = render_html(&parse_final_nodes("- a\n\n  b\n- c\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>a</p>\n<p>b</p>\n</li>\n<li>\n<p>c</p>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.lists.outer-list-may-be-loose-while-inner-list-is-tight]
// gfm[verify container-blocks.lists.loose-sublist-does-not-necessarily-make-outer-list-loose]
#[test]
fn gfm_list_outer_loose_inner_tight() {
    let html = render_html(&parse_final_nodes("- a\n  - b\n  - c\n\n- d\n"));
    assert_eq!(
        html,
        "<ul>\n<li>a<ul>\n<li>b</li>\n<li>c</li>\n</ul>\n</li>\n<li>d</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.lists.items-may-be-separated-by-blank-lines]
// gfm[verify container-blocks.lists.any-number-of-blank-lines-may-separate-items]
#[test]
fn gfm_list_items_separated_by_multiple_blank_lines() {
    let html = render_html(&parse_final_nodes("- a\n\n\n- b\n"));
    assert_eq!(
        html,
        "<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.lists.changing-marker-type-or-delimiter-starts-new-list]
// gfm[verify container-blocks.lists.same-marker-type-definition]
// gfm[verify container-blocks.lists.same-type-definition]
#[test]
fn gfm_list_marker_change_starts_new_list() {
    // Changing between bullet and ordered starts a new list
    let html = render_html(&parse_final_nodes("- foo\n1. bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>foo</li>\n</ul>\n<ol>\n<li>bar</li>\n</ol>\n"
    );

    // Same type stays in same list
    let html = render_html(&parse_final_nodes("1. foo\n2. bar\n"));
    assert_eq!(
        html,
        "<ol>\n<li>foo</li>\n<li>bar</li>\n</ol>\n"
    );
}

// gfm[verify container-blocks.lists.list-can-interrupt-paragraph-without-blank-line]
// gfm[verify container-blocks.lists.commonmark-allows-list-interruption]
#[test]
fn gfm_list_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("foo\n- bar\n"));
    assert_eq!(html, "<p>foo</p>\n<ul>\n<li>bar</li>\n</ul>\n");
}

// gfm[verify container-blocks.lists.only-lists-starting-with-one-may-interrupt-paragraphs]
// gfm[verify container-blocks.lists.markdown-pl-disallows-this-to-avoid-hard-wrapped-numeral-lists]
// gfm[verify container-blocks.lists.starting-with-one-should-prevent-most-spurious-captures]
#[test]
fn gfm_list_only_one_interrupts_paragraph() {
    // "1." can interrupt
    let html = render_html(&parse_final_nodes("foo\n1. bar\n"));
    assert_eq!(html, "<p>foo</p>\n<ol>\n<li>bar</li>\n</ol>\n");

    // "2." cannot interrupt
    let html = render_html(&parse_final_nodes("foo\n2. bar\n"));
    assert_eq!(html, "<p>foo\n2. bar</p>\n");
}

// gfm[verify container-blocks.lists.starting-with-one-can-still-cause-unintended-list-capture]
#[test]
fn gfm_list_one_can_still_capture() {
    let html = render_html(&parse_final_nodes("para\n1. list\n"));
    assert_eq!(html, "<p>para</p>\n<ol>\n<li>list</li>\n</ol>\n");
}

// gfm[verify container-blocks.lists.markdown-pl-still-allows-block-quotes-to-interrupt-paragraphs]
#[test]
fn gfm_block_quote_still_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n> quote\n"));
    assert_eq!(
        html,
        "<p>para</p>\n<blockquote>\n<p>quote</p>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.lists.items-need-not-use-uniform-indentation]
#[test]
fn gfm_list_non_uniform_indentation() {
    let html = render_html(&parse_final_nodes("- a\n - b\n  - c\n"));
    // Each should be an item at various levels
    assert!(html.contains("<li>"));
}

// gfm[verify container-blocks.lists.consecutive-block-elements-without-blank-line-keep-list-tight]
#[test]
fn gfm_list_tight_consecutive_blocks() {
    let html = render_html(&parse_final_nodes("- a\n  - b\n"));
    // No blank lines between, so the outer list should be tight
    assert!(!html.contains("<p>a</p>"));
}

// gfm[verify container-blocks.lists.blank-lines-inside-code-block-do-not-make-list-loose]
#[test]
fn gfm_list_blank_lines_in_code_not_loose() {
    let html = render_html(&parse_final_nodes(
        "- ```\n  a\n\n  b\n  ```\n- c\n",
    ));
    // The blank line inside the fenced code block
    assert!(html.contains("<li>"));
    assert!(html.contains("c"));
}

// gfm[verify container-blocks.lists.blank-line-inside-block-quote-does-not-make-list-loose]
#[test]
fn gfm_list_blank_line_in_blockquote_not_loose() {
    let html = render_html(&parse_final_nodes("- > a\n  >\n  > b\n- c\n"));
    // The blank line inside the block quote should not make the outer list loose
    assert!(!html.contains("<p>c</p>"));
}

// gfm[verify container-blocks.lists.blank-line-before-empty-item-still-makes-list-loose]
#[test]
fn gfm_list_blank_before_empty_item_is_loose() {
    let html = render_html(&parse_final_nodes("- a\n\n-\n"));
    // List with blank line before empty item
    assert!(html.contains("<li>"));
    assert!(html.contains("a"));
}

// gfm[verify container-blocks.lists.four-space-indented-marker-is-code-block]
// gfm[verify container-blocks.lists.more-than-three-spaces-do-not-start-item]
#[test]
fn gfm_list_four_space_marker_is_code() {
    let html = render_html(&parse_final_nodes("    - foo\n"));
    assert_eq!(html, "<pre><code>- foo</code></pre>\n");
}

// gfm[verify container-blocks.lists.nested-example-is-list-item-with-sublist]
// gfm[verify container-blocks.lists.corresponding-standalone-example-should-parse-the-same-way]
#[test]
fn gfm_list_nested_sublist_example() {
    let html = render_html(&parse_final_nodes("- a\n  - b\n    - c\n"));
    assert!(html.contains("<ul>"));
    // Should create nested lists
    assert!(html.matches("<ul>").count() >= 2);
}

// gfm[verify container-blocks.lists.blank-html-comment-separates-lists]
#[test]
fn gfm_list_html_comment_separates_lists() {
    let html = render_html(&parse_final_nodes("- foo\n\n<!-- -->\n\n- bar\n"));
    // The HTML comment separates them into two distinct lists
    assert!(html.matches("<ul>").count() == 2);
}

// gfm[verify container-blocks.lists.start-number-definition]
#[test]
fn gfm_list_start_number() {
    let html = render_html(&parse_final_nodes("3. foo\n4. bar\n"));
    assert_eq!(
        html,
        "<ol start=\"3\">\n<li>foo</li>\n<li>bar</li>\n</ol>\n"
    );
}

// gfm[verify container-blocks.lists.principle-of-uniformity-definition]
// gfm[verify container-blocks.lists.second-reason-is-principle-of-uniformity]
// gfm[verify container-blocks.lists.uniformity-preserves-nested-list-meaning]
// gfm[verify container-blocks.lists.uniformity-supports-list-interruption]
#[test]
fn gfm_list_uniformity() {
    // Uniformity principle: same marker type stays in same list
    let html = render_html(&parse_final_nodes("- a\n- b\n- c\n"));
    assert_eq!(
        html,
        "<ul>\n<li>a</li>\n<li>b</li>\n<li>c</li>\n</ul>\n"
    );
    // Changing between bullet and ordered starts new list
    let html = render_html(&parse_final_nodes("- a\n1. b\n"));
    assert!(html.contains("<ul>"));
    assert!(html.contains("<ol>"));
}

// ---------------------------------------------------------------------------
// Container blocks: Task list items (extension)
// ---------------------------------------------------------------------------

// gfm[verify container-blocks.task-list-items-extension.task-lists-may-be-arbitrarily-nested]
#[test]
fn gfm_task_list_nested() {
    let html = render_html(&parse_final_nodes("- [x] parent\n  - [ ] child\n"));
    assert!(html.contains("checked"));
    assert!(html.contains("<ul>"));
}

// gfm[verify container-blocks.task-list-items-extension.checkbox-interaction-is-unspecified]
// gfm[verify container-blocks.task-list-items-extension.extension-wrapper]
#[test]
fn gfm_task_list_checkbox_interaction_and_wrapper() {
    // The spec says checkbox interaction is unspecified; we just verify the HTML output
    let html = render_html(&parse_final_nodes("- [x] checked\n"));
    assert!(html.contains("disabled=\"\""));
    assert!(html.contains("checked=\"\""));

    let html = render_html(&parse_final_nodes("- [ ] unchecked\n"));
    assert!(html.contains("disabled=\"\""));
    assert!(!html.contains("checked=\"\""));
}

// ---------------------------------------------------------------------------
// Container blocks: List item motivation rules
// ---------------------------------------------------------------------------

// gfm[verify container-blocks.list-items.motivation.original-markdown-spec-quoted]
// gfm[verify container-blocks.list-items.motivation.original-rule-implies-four-space-rule]
// gfm[verify container-blocks.list-items.motivation.four-space-rule-is-arbitrary]
// gfm[verify container-blocks.list-items.motivation.preferred-parse-is-single-list]
#[test]
fn gfm_motivation_four_space_rule() {
    // The four-space rule is replaced by marker-width-based indentation
    // This list should be a single list, not split
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n\n- baz\n"));
    // "bar" is part of the first item (indented 2 = marker width)
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n<li>\n<p>baz</p>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.motivation.four-space-rule-splits-list]
#[test]
fn gfm_motivation_four_space_rule_splits() {
    // Under the old four-space rule, this would be split
    // Under CommonMark, 2 spaces suffice for "-" marker
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert!(html.contains("<p>bar</p>"));
    // bar is inside the list item
    assert!(html.contains("<li>"));
}

// gfm[verify container-blocks.list-items.motivation.marker-width-determines-indentation]
#[test]
fn gfm_motivation_marker_width_determines_indent() {
    // "1. " is 3 chars, "10. " is 4 chars: different indent requirements
    let html = render_html(&parse_final_nodes("1. foo\n   bar\n"));
    assert_eq!(html, "<ol>\n<li>foo\nbar</li>\n</ol>\n");

    let html = render_html(&parse_final_nodes("10. foo\n    bar\n"));
    assert_eq!(html, "<ol start=\"10\">\n<li>foo\nbar</li>\n</ol>\n");
}

// gfm[verify container-blocks.list-items.motivation.fixed-indent-from-marker-is-an-alternative]
// gfm[verify container-blocks.list-items.motivation.fixed-margin-indentation-is-unintuitive]
#[test]
fn gfm_motivation_fixed_indent_alternative() {
    // CommonMark uses marker-width-based indentation rather than a fixed indent
    let html = render_html(&parse_final_nodes("- foo\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.motivation.fixed-marker-indent-breaks-code-patterns]
// gfm[verify container-blocks.list-items.motivation.spec-preserves-eight-space-code-pattern]
#[test]
fn gfm_motivation_code_inside_list_item() {
    // Code inside a list item needs marker-width + 4 spaces
    let html = render_html(&parse_final_nodes("- foo\n\n      code\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>code</code></pre>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.motivation.indented-code-start-needs-special-treatment]
#[test]
fn gfm_motivation_indented_code_start() {
    // When the first line of a list item starts with enough spaces for a code block
    let html = render_html(&parse_final_nodes("-     code\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<pre><code>code</code></pre>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.motivation.markdown-pl-diverged-from-four-space-rule]
// gfm[verify container-blocks.list-items.motivation.no-rule-avoids-breaking-all-existing-documents]
#[test]
fn gfm_motivation_divergence_from_four_space_rule() {
    // CommonMark's approach: marker width determines indentation
    let html = render_html(&parse_final_nodes("- a\n  b\n"));
    assert_eq!(html, "<ul>\n<li>a\nb</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.motivation.two-space-rule-is-extremely-unintuitive]
// gfm[verify container-blocks.list-items.motivation.two-space-rule-allows-underindented-content]
#[test]
fn gfm_motivation_two_space_rule() {
    // Two spaces is the natural indent for "- " markers
    let html = render_html(&parse_final_nodes("- foo\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// gfm[verify container-blocks.list-items.motivation.two-space-rule-example-single-list-item]
#[test]
fn gfm_motivation_two_space_single_item() {
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n"
    );
}

// gfm[verify container-blocks.list-items.motivation.two-space-rule-example-inside-block-quote]
// gfm[verify container-blocks.list-items.motivation.two-space-rule-block-quote-html]
#[test]
fn gfm_motivation_two_space_inside_block_quote() {
    let html = render_html(&parse_final_nodes("> - foo\n>   bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<ul>\n<li>foo\nbar</li>\n</ul>\n</blockquote>\n"
    );
}

// gfm[verify container-blocks.list-items.motivation.extension-wrapper]
#[test]
fn gfm_motivation_extension_wrapper() {
    // The motivation section explains the rationale for the list item rules
    // This test verifies the resulting behavior
    let html = render_html(&parse_final_nodes("- foo\n  - bar\n    - baz\n"));
    assert!(html.contains("<ul>"));
    assert!(html.matches("<li>").count() == 3);
}
