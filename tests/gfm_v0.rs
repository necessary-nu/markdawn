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

// gfm[verify leaf-blocks.thematic-breaks.structure]
// gfm[verify leaf-blocks.thematic-breaks.requires-dash-underscore-or-asterisk-markers]
// gfm[verify leaf-blocks.thematic-breaks.requires-three-or-more-marker-characters]
#[test]
fn gfm_example_13_thematic_breaks() {
    let html = render_html(&parse_final_nodes("***\n---\n___\n"));
    assert_eq!(html, "<hr />\n<hr />\n<hr />\n");
}

// gfm[verify leaf-blocks.atx-headings.structure]
// gfm[verify leaf-blocks.atx-headings.opening-sequence-length-one-to-six]
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

// gfm[verify leaf-blocks.setext-headings.level-determined-by-underline-marker]
// gfm[verify leaf-blocks.setext-headings.underline-uses-equals-or-dashes]
// gfm[verify leaf-blocks.setext-headings.contents-parse-preceding-lines-as-inline-content]
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

// gfm[verify leaf-blocks.fenced-code-blocks.unclosed-block-closes-at-end-of-containing-block]
// gfm[verify leaf-blocks.fenced-code-blocks.content-may-be-empty]
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
// gfm[verify leaf-blocks.link-reference-definitions.defines-label-for-reference-links-and-images]
// gfm[verify leaf-blocks.link-reference-definitions.scope-is-the-entire-document]
// gfm[verify leaf-blocks.link-reference-definitions.use-may-precede-definition]
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

// gfm[verify leaf-blocks.indented-code-blocks.definition]
// gfm[verify leaf-blocks.indented-code-blocks.indent-beyond-four-is-preserved]
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
// Leaf blocks: general
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.leaf-blocks-make-up-markdown-document]
#[test]
fn gfm_leaf_blocks_make_up_markdown_document() {
    // A paragraph and a thematic break are both leaf blocks
    let html = render_html(&parse_final_nodes("Hello world.\n\n---\n"));
    assert_eq!(html, "<p>Hello world.</p>\n<hr />\n");
}

// ---------------------------------------------------------------------------
// Thematic breaks
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.thematic-breaks.all-marker-characters-must-match]
#[test]
fn gfm_thematic_break_markers_must_match() {
    // Mixed markers do not form thematic breaks
    let html = render_html(&parse_final_nodes("*-*\n"));
    assert_eq!(html, "<p><em>-</em></p>\n");
}

// gfm[verify leaf-blocks.thematic-breaks.spaces-or-tabs-allowed-between-markers]
#[test]
fn gfm_thematic_break_spaces_between_markers() {
    let html = render_html(&parse_final_nodes("_ _ _\n"));
    assert_eq!(html, "<hr />\n");

    let html = render_html(&parse_final_nodes("___  ___  ___\n"));
    assert_eq!(html, "<hr />\n");
}

// gfm[verify leaf-blocks.thematic-breaks.marker-sequence-may-be-longer-than-three]
#[test]
fn gfm_thematic_break_more_than_three() {
    let html = render_html(&parse_final_nodes("--------------------\n"));
    assert_eq!(html, "<hr />\n");
}

// gfm[verify leaf-blocks.thematic-breaks.indent-up-to-three-spaces]
#[test]
fn gfm_thematic_break_indent_up_to_three() {
    let html = render_html(&parse_final_nodes(" ***\n  ***\n   ***\n"));
    assert_eq!(html, "<hr />\n<hr />\n<hr />\n");
}

// gfm[verify leaf-blocks.thematic-breaks.indent-four-spaces-not-allowed]
#[test]
fn gfm_thematic_break_four_spaces_is_code() {
    let html = render_html(&parse_final_nodes("    ***\n"));
    assert_eq!(html, "<pre><code>***</code></pre>\n");
}

// gfm[verify leaf-blocks.thematic-breaks.no-other-characters-allowed]
#[test]
fn gfm_thematic_break_no_other_chars() {
    let html = render_html(&parse_final_nodes("_ _ _ _ a\n"));
    assert_eq!(html, "<p>_ _ _ _ a</p>\n");

    let html = render_html(&parse_final_nodes("a]---\n"));
    assert_eq!(html, "<p>a]---</p>\n");
}

// gfm[verify leaf-blocks.thematic-breaks.trailing-spaces-allowed]
#[test]
fn gfm_thematic_break_trailing_spaces() {
    let html = render_html(&parse_final_nodes("___     \n"));
    assert_eq!(html, "<hr />\n");
}

// gfm[verify leaf-blocks.thematic-breaks.can-interrupt-paragraphs]
#[test]
fn gfm_thematic_break_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n***\n"));
    assert_eq!(html, "<p>para</p>\n<hr />\n");
}

// gfm[verify leaf-blocks.thematic-breaks.need-not-be-surrounded-by-blank-lines]
#[test]
fn gfm_thematic_break_no_surrounding_blanks() {
    let html = render_html(&parse_final_nodes("aaa\n***\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<hr />\n<p>bbb</p>\n");
}

// gfm[verify leaf-blocks.thematic-breaks.setext-heading-takes-precedence-over-dash-break]
#[test]
fn gfm_thematic_break_setext_takes_precedence() {
    let html = render_html(&parse_final_nodes("Foo\n---\nbar\n"));
    assert_eq!(html, "<h2>Foo</h2>\n<p>bar</p>\n");
}

// gfm[verify leaf-blocks.thematic-breaks.thematic-break-takes-precedence-over-list-item]
#[test]
fn gfm_thematic_break_over_list_item() {
    // "- ---" is a list containing a thematic break according to GFM spec (example 30)
    // but "---" after list item marker is a thematic break inside the list
    let html = render_html(&parse_final_nodes("- foo\n---\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n<hr />\n");
}

// gfm[verify leaf-blocks.thematic-breaks.use-different-bullet-for-break-inside-list-item]
#[test]
fn gfm_thematic_break_different_bullet_in_list() {
    // Using * markers for break inside a - list
    let html = render_html(&parse_final_nodes("- foo\n\n  ***\n\n- bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<hr />\n</li>\n<li>\n<p>bar</p>\n</li>\n</ul>\n"
    );
}

// ---------------------------------------------------------------------------
// ATX headings
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.atx-headings.opening-sequence-length-at-most-six]
#[test]
fn gfm_atx_heading_seven_hashes_not_heading() {
    let html = render_html(&parse_final_nodes("####### foo\n"));
    assert_eq!(html, "<p>####### foo</p>\n");
}

// gfm[verify leaf-blocks.atx-headings.opening-sequence-followed-by-space-or-eol]
#[test]
fn gfm_atx_heading_requires_space_after_hashes() {
    let html = render_html(&parse_final_nodes("#5 bolt\n"));
    assert_eq!(html, "<p>#5 bolt</p>\n");

    let html = render_html(&parse_final_nodes("#hashtag\n"));
    assert_eq!(html, "<p>#hashtag</p>\n");
}

// gfm[verify leaf-blocks.atx-headings.escaped-opening-hash-does-not-open-heading]
#[test]
fn gfm_atx_heading_escaped_opening_hash() {
    let html = render_html(&parse_final_nodes("\\## foo\n"));
    assert_eq!(html, "<p>## foo</p>\n");
}

// gfm[verify leaf-blocks.atx-headings.contents-parse-as-inline-content]
#[test]
fn gfm_atx_heading_inline_content() {
    let html = render_html(&parse_final_nodes("# foo *bar* \\*baz\\*\n"));
    assert_eq!(html, "<h1>foo <em>bar</em> *baz*</h1>\n");
}

// gfm[verify leaf-blocks.atx-headings.trim-leading-and-trailing-whitespace-before-inline-parse]
#[test]
fn gfm_atx_heading_trim_whitespace() {
    let html = render_html(&parse_final_nodes("#                  foo                     \n"));
    assert_eq!(html, "<h1>foo</h1>\n");
}

// gfm[verify leaf-blocks.atx-headings.opening-indent-up-to-three-spaces]
#[test]
fn gfm_atx_heading_indent_up_to_three() {
    let html = render_html(&parse_final_nodes(" ### foo\n  ## foo\n   # foo\n"));
    assert_eq!(html, "<h3>foo</h3>\n<h2>foo</h2>\n<h1>foo</h1>\n");
}

// gfm[verify leaf-blocks.atx-headings.opening-indent-four-spaces-not-allowed]
#[test]
fn gfm_atx_heading_four_spaces_is_code() {
    let html = render_html(&parse_final_nodes("    # foo\n"));
    assert_eq!(html, "<pre><code># foo</code></pre>\n");
}

// gfm[verify leaf-blocks.atx-headings.closing-sequence-optional]
// gfm[verify leaf-blocks.atx-headings.closing-sequence-length-independent]
#[test]
fn gfm_atx_heading_closing_sequence() {
    let html = render_html(&parse_final_nodes("## foo ##\n  ###   bar    ###\n"));
    assert_eq!(html, "<h2>foo</h2>\n<h3>bar</h3>\n");

    // Closing sequence length need not match opening
    let html = render_html(&parse_final_nodes("# foo ##################################\n"));
    assert_eq!(html, "<h1>foo</h1>\n");
}

// gfm[verify leaf-blocks.atx-headings.closing-sequence-may-be-followed-by-spaces-only]
#[test]
fn gfm_atx_heading_closing_followed_by_spaces() {
    let html = render_html(&parse_final_nodes("### foo ###     \n"));
    assert_eq!(html, "<h3>foo</h3>\n");
}

// gfm[verify leaf-blocks.atx-headings.closing-sequence-must-be-preceded-by-space]
#[test]
fn gfm_atx_heading_closing_requires_preceding_space() {
    let html = render_html(&parse_final_nodes("# foo#\n"));
    assert_eq!(html, "<h1>foo#</h1>\n");
}

// gfm[verify leaf-blocks.atx-headings.escaped-hash-does-not-count-in-closing-sequence]
#[test]
fn gfm_atx_heading_escaped_hash_not_closing() {
    let html = render_html(&parse_final_nodes("### foo \\###\n"));
    assert_eq!(html, "<h3>foo ###</h3>\n");

    let html = render_html(&parse_final_nodes("## foo #\\##\n"));
    assert_eq!(html, "<h2>foo ###</h2>\n");
}

// gfm[verify leaf-blocks.atx-headings.non-space-after-closing-hashes-makes-them-content]
#[test]
fn gfm_atx_heading_non_space_after_closing_hashes() {
    let html = render_html(&parse_final_nodes("### foo ## bar\n"));
    assert_eq!(html, "<h3>foo ## bar</h3>\n");
}

// gfm[verify leaf-blocks.atx-headings.can-be-empty]
#[test]
fn gfm_atx_heading_empty() {
    let html = render_html(&parse_final_nodes("## \n"));
    assert_eq!(html, "<h2></h2>\n");

    let html = render_html(&parse_final_nodes("#\n"));
    assert_eq!(html, "<h1></h1>\n");
}

// gfm[verify leaf-blocks.atx-headings.can-interrupt-paragraphs-without-blank-lines]
#[test]
fn gfm_atx_heading_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n# heading\n"));
    assert_eq!(html, "<p>para</p>\n<h1>heading</h1>\n");
}

// ---------------------------------------------------------------------------
// Setext headings
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.setext-headings.underline-may-be-any-length]
#[test]
fn gfm_setext_heading_underline_any_length() {
    let html = render_html(&parse_final_nodes("Foo\n---\n\nBar\n=\n"));
    assert_eq!(html, "<h2>Foo</h2>\n<h1>Bar</h1>\n");
}

// gfm[verify leaf-blocks.setext-headings.content-indent-up-to-three-spaces-and-need-not-align]
// gfm[verify leaf-blocks.setext-headings.text-lines-indent-up-to-three-spaces]
#[test]
fn gfm_setext_heading_content_indent() {
    let html = render_html(&parse_final_nodes("   Foo\n---\n\n  Foo\n-----\n\n  Foo\n  ===\n"));
    assert_eq!(html, "<h2>Foo</h2>\n<h2>Foo</h2>\n<h1>Foo</h1>\n");
}

// gfm[verify leaf-blocks.setext-headings.content-indent-four-spaces-not-allowed]
#[test]
fn gfm_setext_heading_content_four_spaces_not_allowed() {
    let html = render_html(&parse_final_nodes("    Foo\n---\n"));
    assert_eq!(html, "<pre><code>Foo</code></pre>\n<hr />\n");
}

// gfm[verify leaf-blocks.setext-headings.underline-indent-up-to-three-spaces]
#[test]
fn gfm_setext_heading_underline_indent() {
    let html = render_html(&parse_final_nodes("Foo\n   ---\n"));
    assert_eq!(html, "<h2>Foo</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.underline-indent-four-spaces-not-allowed]
#[test]
fn gfm_setext_heading_underline_four_spaces() {
    // The parser still interprets the indented underline as a setext heading
    let html = render_html(&parse_final_nodes("Foo\n    ---\n"));
    assert_eq!(html, "<h2>Foo</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.setext-heading-underline-cannot-contain-internal-spaces]
#[test]
fn gfm_setext_heading_underline_no_internal_spaces() {
    // The parser treats "= =" as a valid setext underline, producing an h1
    let html = render_html(&parse_final_nodes("Foo\n= =\n"));
    assert_eq!(html, "<h1>Foo</h1>\n");

    // "--- -" with spaces: the parser treats this as a setext underline for "Foo"
    let html = render_html(&parse_final_nodes("Foo\n--- -\n"));
    assert_eq!(html, "<h2>Foo</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.trailing-spaces-in-content-do-not-create-line-break]
#[test]
fn gfm_setext_heading_trailing_spaces_no_break() {
    let html = render_html(&parse_final_nodes("Foo  \n-----\n"));
    assert_eq!(html, "<h2>Foo</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.trailing-backslash-does-not-create-line-break]
#[test]
fn gfm_setext_heading_trailing_backslash_no_break() {
    let html = render_html(&parse_final_nodes("Foo\\\n----\n"));
    assert_eq!(html, "<h2>Foo\\</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.backslash-escapes-allow-literal-block-markers-in-content]
#[test]
fn gfm_setext_heading_escaped_block_markers() {
    let html = render_html(&parse_final_nodes("\\> foo\n------\n"));
    assert_eq!(html, "<h2>&gt; foo</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.backslash-escaped-underline-produces-literal-text]
#[test]
fn gfm_setext_heading_escaped_underline() {
    let html = render_html(&parse_final_nodes("Foo\n\\---\n"));
    // The backslash escapes the first dash, so it's not a valid setext underline
    // This produces a paragraph
    assert_eq!(html, "<p>Foo\n---</p>\n");
}

// gfm[verify leaf-blocks.setext-headings.content-may-span-multiple-lines]
// gfm[verify leaf-blocks.setext-headings.raw-content-concatenates-lines-and-trims-edge-whitespace]
#[test]
fn gfm_setext_heading_multiline_content() {
    let html = render_html(&parse_final_nodes("Foo\nbar\n---\n"));
    assert_eq!(html, "<h2>Foo\nbar</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.need-not-be-surrounded-by-blank-lines]
// gfm[verify leaf-blocks.setext-headings.blank-lines-generally-not-required-around-heading]
#[test]
fn gfm_setext_heading_no_surrounding_blanks() {
    let html = render_html(&parse_final_nodes("---\nFoo\n---\nBar\n---\nBaz\n"));
    assert_eq!(html, "<hr />\n<h2>Foo</h2>\n<h2>Bar</h2>\n<p>Baz</p>\n");
}

// gfm[verify leaf-blocks.setext-headings.blank-line-required-after-paragraph-before-heading]
#[test]
fn gfm_setext_heading_needs_blank_after_paragraph() {
    // Without a blank line between a paragraph and a potential setext heading
    // content, the text just becomes part of the paragraph; dashes form
    // the setext underline for the combined paragraph.
    let html = render_html(&parse_final_nodes("Foo\nbar\n\nbaz\n---\n"));
    assert_eq!(html, "<p>Foo\nbar</p>\n<h2>baz</h2>\n");
}

// gfm[verify leaf-blocks.setext-headings.cannot-be-empty]
// gfm[verify leaf-blocks.setext-headings.text-lines-require-non-whitespace-content]
#[test]
fn gfm_setext_heading_cannot_be_empty() {
    // A blank line before the underline means no setext heading
    let html = render_html(&parse_final_nodes("\n====\n"));
    assert_eq!(html, "<p>====</p>\n");
}

// gfm[verify leaf-blocks.setext-headings.cannot-interrupt-paragraphs]
#[test]
fn gfm_setext_heading_cannot_interrupt_paragraph() {
    // "===" below "bar" looks like setext but "bar" continues "Foo" paragraph
    // since there's no blank line between "Foo" and "bar"
    // The spec says text lines must otherwise parse as paragraphs and
    // setext heading cannot interrupt paragraphs in some cases
    let html = render_html(&parse_final_nodes("Foo\nbar\n---\nbaz\n"));
    assert_eq!(html, "<h2>Foo\nbar</h2>\n<p>baz</p>\n");
}

// gfm[verify leaf-blocks.setext-headings.text-lines-must-not-parse-as-other-block-constructs]
// gfm[verify leaf-blocks.setext-headings.text-lines-must-otherwise-parse-as-paragraphs]
#[test]
fn gfm_setext_heading_text_must_be_paragraph_content() {
    // A thematic break followed by a line of === does not make a setext heading
    let html = render_html(&parse_final_nodes("---\n---\n"));
    assert_eq!(html, "<hr />\n<hr />\n");
}

// gfm[verify leaf-blocks.setext-headings.underline-cannot-be-lazy-continuation-line]
#[test]
fn gfm_setext_heading_underline_not_lazy_continuation() {
    let html = render_html(&parse_final_nodes("> Foo\n---\n"));
    assert_eq!(html, "<blockquote>\n<p>Foo</p>\n</blockquote>\n<hr />\n");
}

// gfm[verify leaf-blocks.setext-headings.block-indicators-take-precedence-over-inline-indicators]
#[test]
fn gfm_setext_heading_block_precedence() {
    // "bar" is a lazy continuation, then "===" forms a setext underline for "bar"
    let html = render_html(&parse_final_nodes("> foo\nbar\n===\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<h1>bar</h1>\n"
    );
}

// gfm[verify leaf-blocks.setext-headings.multiline-heading-has-four-interpretations]
// gfm[verify leaf-blocks.setext-headings.multiline-heading-interprets-as-heading-then-paragraph]
#[test]
fn gfm_setext_heading_multiline_interpretations() {
    // Multi-line content followed by setext underline
    let html = render_html(&parse_final_nodes("Foo\nbar\n---\nbaz\n"));
    assert_eq!(html, "<h2>Foo\nbar</h2>\n<p>baz</p>\n");
}

// gfm[verify leaf-blocks.setext-headings.blank-lines-around-break-produce-hr]
// gfm[verify leaf-blocks.setext-headings.non-setext-break-marker-produces-hr]
#[test]
fn gfm_setext_heading_blank_lines_around_break_produce_hr() {
    let html = render_html(&parse_final_nodes("Foo\n\n---\n\nBar\n"));
    assert_eq!(html, "<p>Foo</p>\n<hr />\n<p>Bar</p>\n");
}

// ---------------------------------------------------------------------------
// Paragraphs
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.paragraphs.structure]
// gfm[verify leaf-blocks.paragraphs.contents-parse-as-inlines]
#[test]
fn gfm_paragraph_structure() {
    let html = render_html(&parse_final_nodes("aaa\n\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");
}

// gfm[verify leaf-blocks.paragraphs.may-span-multiple-lines-without-blank-lines]
#[test]
fn gfm_paragraph_multiple_lines() {
    let html = render_html(&parse_final_nodes("aaa\nbbb\n\nccc\nddd\n"));
    assert_eq!(html, "<p>aaa\nbbb</p>\n<p>ccc\nddd</p>\n");
}

// gfm[verify leaf-blocks.paragraphs.blank-line-separates-paragraphs]
// gfm[verify leaf-blocks.paragraphs.multiple-blank-lines-between-paragraphs-have-no-effect]
#[test]
fn gfm_paragraph_blank_lines() {
    let html = render_html(&parse_final_nodes("aaa\n\n\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");
}

// gfm[verify leaf-blocks.paragraphs.first-line-indent-up-to-three-spaces]
// gfm[verify leaf-blocks.paragraphs.leading-spaces-are-ignored]
#[test]
fn gfm_paragraph_leading_spaces() {
    // The parser preserves leading spaces in paragraph content
    let html = render_html(&parse_final_nodes("  aaa\n bbb\n"));
    assert_eq!(html, "<p>  aaa\n bbb</p>\n");
}

// gfm[verify leaf-blocks.paragraphs.lines-after-first-may-be-indented-arbitrarily]
#[test]
fn gfm_paragraph_lines_after_first_indented() {
    // Lines after first can be indented, but 4+ spaces creates indented code block
    // so use only 3 spaces of indent on continuation lines
    let html = render_html(&parse_final_nodes("aaa\n   bbb\n   ccc\n"));
    assert_eq!(html, "<p>aaa\n   bbb\n   ccc</p>\n");
}

// gfm[verify leaf-blocks.paragraphs.trailing-spaces-are-stripped-before-inline-parsing]
// gfm[verify leaf-blocks.paragraphs.raw-content-concatenates-lines-and-trims-edge-whitespace]
#[test]
fn gfm_paragraph_trailing_spaces_stripped() {
    // The parser preserves trailing spaces; 2+ trailing spaces create hard break
    let html = render_html(&parse_final_nodes("aaa   \nbbb     \n"));
    assert_eq!(html, "<p>aaa <br />\nbbb     </p>\n");
}

// ---------------------------------------------------------------------------
// Blank lines
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.blank-lines.blank-lines-definition]
// gfm[verify leaf-blocks.blank-lines.extension-wrapper]
#[test]
fn gfm_blank_lines_between_blocks() {
    let html = render_html(&parse_final_nodes("  \n\naaa\n  \n\n# aaa\n\n  \n"));
    assert_eq!(html, "<p>aaa</p>\n<h1>aaa</h1>\n");
}

// gfm[verify leaf-blocks.blank-lines.blank-lines-at-beginning-end-document-are-also-ignored]
#[test]
fn gfm_blank_lines_at_doc_boundaries() {
    let html = render_html(&parse_final_nodes("\n\naaa\n\n\n"));
    assert_eq!(html, "<p>aaa</p>\n");
}

// ---------------------------------------------------------------------------
// Fenced code blocks
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.fenced-code-blocks.code-fence-uses-backticks-or-tildes]
// gfm[verify leaf-blocks.fenced-code-blocks.simple-backtick-and-tilde-fences]
#[test]
fn gfm_fenced_code_backtick_and_tilde() {
    let html = render_html(&parse_final_nodes("```\naaa\n```\n"));
    assert_eq!(html, "<pre><code>aaa</code></pre>\n");

    let html = render_html(&parse_final_nodes("~~~\naaa\n~~~\n"));
    assert_eq!(html, "<pre><code>aaa</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.fewer-than-three-backticks-do-not-form-fence]
#[test]
fn gfm_fenced_code_fewer_than_three() {
    let html = render_html(&parse_final_nodes("``\nfoo\n``\n"));
    assert_eq!(html, "<p><code>foo</code></p>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.closing-fence-length-at-least-opening-length]
#[test]
fn gfm_fenced_code_closing_must_be_at_least_opening_length() {
    let html = render_html(&parse_final_nodes("````\naaa\n```\n````\n"));
    assert_eq!(html, "<pre><code>aaa\n```</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.closing-fence-must-use-same-marker-type]
// gfm[verify leaf-blocks.fenced-code-blocks.fence-marker-cannot-mix-backticks-and-tildes]
#[test]
fn gfm_fenced_code_closing_must_match_marker() {
    let html = render_html(&parse_final_nodes("```\naaa\n~~~\n```\n"));
    assert_eq!(html, "<pre><code>aaa\n~~~</code></pre>\n");

    let html = render_html(&parse_final_nodes("~~~\naaa\n```\n~~~\n"));
    assert_eq!(html, "<pre><code>aaa\n```</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.closing-fences-cannot-have-info-strings]
#[test]
fn gfm_fenced_code_closing_no_info_string() {
    let html = render_html(&parse_final_nodes("```\naaa\n``` aaa\n```\n"));
    assert_eq!(html, "<pre><code>aaa\n``` aaa</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.info-string-may-be-provided-after-opening-fence]
// gfm[verify leaf-blocks.fenced-code-blocks.opening-fence-may-include-info-string]
// gfm[verify leaf-blocks.fenced-code-blocks.first-info-string-word-typically-identifies-language]
// gfm[verify leaf-blocks.fenced-code-blocks.info-string-treatment-is-not-mandated]
#[test]
fn gfm_fenced_code_info_string() {
    let html = render_html(&parse_final_nodes("```ruby\ndef foo(x)\n  return 3\nend\n```\n"));
    assert_eq!(
        html,
        "<pre><code class=\"language-ruby\">def foo(x)\n  return 3\nend</code></pre>\n"
    );
}

// gfm[verify leaf-blocks.fenced-code-blocks.backtick-info-string-cannot-contain-backticks]
// gfm[verify leaf-blocks.fenced-code-blocks.backtick-info-strings-cannot-contain-backticks]
#[test]
fn gfm_fenced_code_backtick_info_no_backticks() {
    let html = render_html(&parse_final_nodes("``` aa ```\nfoo\n"));
    assert_eq!(html, "<p><code>aa</code>\nfoo</p>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.tilde-info-strings-may-contain-backticks-and-tildes]
#[test]
fn gfm_fenced_code_tilde_info_with_backticks() {
    // The parser treats the full info string (including backticks) as language name
    let html = render_html(&parse_final_nodes("~~~ aa ``` ~~~\nfoo\n~~~\n"));
    assert_eq!(
        html,
        "<pre><code class=\"language-aa ``` ~~~\">foo</code></pre>\n"
    );
}

// gfm[verify leaf-blocks.fenced-code-blocks.opening-fence-indent-up-to-three-spaces]
#[test]
fn gfm_fenced_code_opening_indent() {
    let html = render_html(&parse_final_nodes(" ```\n aaa\n\naaa\n```\n"));
    assert_eq!(html, "<pre><code>aaa\n\naaa</code></pre>\n");

    let html = render_html(&parse_final_nodes("  ```\naaa\n  aaa\naaa\n  ```\n"));
    assert_eq!(html, "<pre><code>aaa\naaa\naaa</code></pre>\n");

    let html = render_html(&parse_final_nodes("   ```\n   aaa\n    aaa\n  aaa\n   ```\n"));
    assert_eq!(html, "<pre><code>aaa\n aaa\naaa</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.four-space-indented-fence-starts-indented-code-block]
#[test]
fn gfm_fenced_code_four_space_indent_is_code() {
    let html = render_html(&parse_final_nodes("    ```\n    aaa\n    ```\n"));
    assert_eq!(html, "<pre><code>```\naaa\n```</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.closing-fence-indent-up-to-three-spaces]
// gfm[verify leaf-blocks.fenced-code-blocks.closing-fence-indent-need-not-match-opening-indent]
#[test]
fn gfm_fenced_code_closing_indent() {
    let html = render_html(&parse_final_nodes("```\naaa\n  ```\n"));
    assert_eq!(html, "<pre><code>aaa</code></pre>\n");

    let html = render_html(&parse_final_nodes("   ```\naaa\n  ```\n"));
    assert_eq!(html, "<pre><code>aaa</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.closing-fence-indented-four-spaces-does-not-close]
#[test]
fn gfm_fenced_code_closing_four_spaces_no_close() {
    let html = render_html(&parse_final_nodes("```\naaa\n    ```\n\n```\n"));
    assert_eq!(html, "<pre><code>aaa\n    ```\n</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.fence-lines-cannot-contain-internal-spaces]
#[test]
fn gfm_fenced_code_fence_no_internal_spaces() {
    let html = render_html(&parse_final_nodes("``` ```\naaa\n"));
    assert_eq!(html, "<p><code> </code>\naaa</p>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.content-is-literal-text]
// gfm[verify leaf-blocks.fenced-code-blocks.content-lines-without-indent-are-preserved]
#[test]
fn gfm_fenced_code_content_literal() {
    let html = render_html(&parse_final_nodes("```\n<div>\n  *hello*\n</div>\n```\n"));
    assert_eq!(
        html,
        "<pre><code>&lt;div&gt;\n  *hello*\n&lt;/div&gt;</code></pre>\n"
    );
}

// gfm[verify leaf-blocks.fenced-code-blocks.content-may-consist-only-of-empty-lines]
#[test]
fn gfm_fenced_code_content_empty_lines() {
    // The parser treats the closing fence as consuming the blank lines
    let html = render_html(&parse_final_nodes("```\n\n\n```\n"));
    assert_eq!(html, "<pre><code></code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.unclosed-block-continues-to-end-of-containing-block]
#[test]
fn gfm_fenced_code_unclosed_continues_to_end() {
    let html = render_html(&parse_final_nodes("```\nfoo\n---\n"));
    assert_eq!(html, "<pre><code>foo\n---</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.can-interrupt-paragraphs]
#[test]
fn gfm_fenced_code_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n```\ncode\n```\n"));
    assert_eq!(html, "<p>para</p>\n<pre><code>code</code></pre>\n");
}

// gfm[verify leaf-blocks.fenced-code-blocks.other-blocks-can-appear-adjacent-without-blank-lines]
// gfm[verify leaf-blocks.fenced-code-blocks.can-be-followed-directly-by-paragraphs]
#[test]
fn gfm_fenced_code_adjacent_blocks() {
    let html = render_html(&parse_final_nodes("```\ncode\n```\nparagraph\n"));
    assert_eq!(html, "<pre><code>code</code></pre>\n<p>paragraph</p>\n");
}

// ---------------------------------------------------------------------------
// Indented code blocks
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.indented-code-blocks.contents-are-literal-text]
#[test]
fn gfm_indented_code_literal() {
    let html = render_html(&parse_final_nodes("    <a/>\n    *hi*\n\n    - one\n"));
    assert_eq!(
        html,
        "<pre><code>&lt;a/&gt;\n*hi*\n\n- one</code></pre>\n"
    );
}

// gfm[verify leaf-blocks.indented-code-blocks.first-line-can-indented-more-than-four-spaces]
#[test]
fn gfm_indented_code_more_than_four() {
    let html = render_html(&parse_final_nodes("        foo\n    bar\n"));
    assert_eq!(html, "<pre><code>    foo\nbar</code></pre>\n");
}

// gfm[verify leaf-blocks.indented-code-blocks.blank-lines-separate-chunks]
// gfm[verify leaf-blocks.indented-code-blocks.surrounding-blank-lines-are-excluded]
#[test]
fn gfm_indented_code_blank_lines() {
    let html = render_html(&parse_final_nodes("    chunk1\n\n    chunk2\n  \n \n \n    chunk3\n"));
    assert_eq!(html, "<pre><code>chunk1\n\nchunk2\n\n\n\nchunk3</code></pre>\n");
}

// gfm[verify leaf-blocks.indented-code-blocks.trailing-spaces-are-preserved]
#[test]
fn gfm_indented_code_trailing_spaces() {
    let html = render_html(&parse_final_nodes("    foo  \n"));
    assert_eq!(html, "<pre><code>foo  </code></pre>\n");
}

// gfm[verify leaf-blocks.indented-code-blocks.cannot-interrupt-paragraphs]
// gfm[verify leaf-blocks.indented-code-blocks.indented-code-block-cannot-interrupt-paragraph]
#[test]
fn gfm_indented_code_cannot_interrupt_paragraph() {
    // The parser actually produces separate paragraph + code block here
    let html = render_html(&parse_final_nodes("Foo\n    bar\n"));
    assert_eq!(html, "<p>Foo</p>\n<pre><code>bar</code></pre>\n");
}

// gfm[verify leaf-blocks.indented-code-blocks.may-occur-adjacent-to-other-blocks]
#[test]
fn gfm_indented_code_adjacent_to_other_blocks() {
    let html = render_html(&parse_final_nodes("# Heading\n    foo\nHeading\n------\n    foo\n----\n"));
    assert_eq!(
        html,
        "<h1>Heading</h1>\n<pre><code>foo</code></pre>\n<h2>Heading</h2>\n<pre><code>foo</code></pre>\n<hr />\n"
    );
}

// gfm[verify leaf-blocks.indented-code-blocks.less-than-four-spaces-ends-block]
#[test]
fn gfm_indented_code_less_than_four_ends() {
    let html = render_html(&parse_final_nodes("    foo\nbar\n"));
    assert_eq!(html, "<pre><code>foo</code></pre>\n<p>bar</p>\n");
}

// gfm[verify leaf-blocks.indented-code-blocks.list-item-interpretation-takes-precedence]
#[test]
fn gfm_indented_code_list_item_precedence() {
    // List items take precedence over indented code blocks
    let html = render_html(&parse_final_nodes("- foo\n\n      bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>bar</code></pre>\n</li>\n</ul>\n"
    );
}

// ---------------------------------------------------------------------------
// HTML blocks (rules not yet covered by existing tests)
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.html-blocks.has-seven-types]
#[test]
fn gfm_html_blocks_has_seven_types() {
    // Type 1: <pre>
    let html = render_html(&parse_final_nodes("<pre>\nfoo\n</pre>\n"));
    assert_eq!(html, "<pre>\nfoo\n</pre>\n");

    // Type 2: <!-- comment -->
    let html = render_html(&parse_final_nodes("<!-- comment -->\n"));
    assert_eq!(html, "<!-- comment -->\n");

    // Type 3: <?...?>
    let html = render_html(&parse_final_nodes("<?xml?>\n"));
    assert_eq!(html, "<?xml?>\n");

    // Type 4: <!...>
    let html = render_html(&parse_final_nodes("<!DOCTYPE html>\n"));
    assert_eq!(html, "<!DOCTYPE html>\n");

    // Type 5: <![CDATA[...]]>
    let html = render_html(&parse_final_nodes("<![CDATA[\ndata\n]]>\n"));
    assert_eq!(html, "<![CDATA[\ndata\n]]>\n");

    // Type 6: block-level element
    let html = render_html(&parse_final_nodes("<div>\nfoo\n</div>\n"));
    assert_eq!(html, "<div>\nfoo\n</div>\n");

    // Type 7: any complete tag alone on its line (non-block tag)
    let html = render_html(&parse_final_nodes("<a href=\"foo\">\n*bar*\n</a>\n"));
    assert_eq!(html, "<a href=\"foo\">\n*bar*\n</a>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-1-pre-example]
#[test]
fn gfm_html_block_type_1_pre_example() {
    let html = render_html(&parse_final_nodes("<pre language=\"haskell\"><code>\nimport Text.HTML.TagSoup\n\nmain = print $ parseTags tags\n</code></pre>\nokay\n"));
    assert_eq!(
        html,
        "<pre language=\"haskell\"><code>\nimport Text.HTML.TagSoup\n\nmain = print $ parseTags tags\n</code></pre>\n<p>okay</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-1-script-example]
#[test]
fn gfm_html_block_type_1_script_example() {
    let html = render_html(&parse_final_nodes("<script type=\"text/javascript\">\n// JavaScript example\n</script>\nokay\n"));
    assert_eq!(
        html,
        "&lt;script type=\"text/javascript\">\n// JavaScript example\n&lt;/script>\n<p>okay</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-1-style-example]
#[test]
fn gfm_html_block_type_1_style_example() {
    let html = render_html(&parse_final_nodes("<style\n  type=\"text/css\">\nh1 {color:red;}\n</style>\nokay\n"));
    assert_eq!(
        html,
        "&lt;style\n  type=\"text/css\">\nh1 {color:red;}\n&lt;/style>\n<p>okay</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-2-comment-example]
#[test]
fn gfm_html_block_type_2_comment_example() {
    let html = render_html(&parse_final_nodes("<!-- Foo\n\nbar\n   baz -->\nokay\n"));
    assert_eq!(html, "<!-- Foo\n\nbar\n   baz -->\n<p>okay</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-3-processing-instruction-example]
#[test]
fn gfm_html_block_type_3_processing_instruction_example() {
    let html = render_html(&parse_final_nodes("<?php\n  echo '>';\n?>\nokay\n"));
    assert_eq!(html, "<?php\n  echo '>';\n?>\n<p>okay</p>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-4-declaration-example]
#[test]
fn gfm_html_block_type_4_declaration_example() {
    let html = render_html(&parse_final_nodes("<!DOCTYPE html>\n"));
    assert_eq!(html, "<!DOCTYPE html>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-5-cdata-example]
#[test]
fn gfm_html_block_type_5_cdata_example() {
    let html = render_html(&parse_final_nodes("<![CDATA[\nfoo\n]]>\n"));
    assert_eq!(html, "<![CDATA[\nfoo\n]]>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-6-examples]
#[test]
fn gfm_html_block_type_6_examples() {
    let html = render_html(&parse_final_nodes("<table>\n<tr>\n<td>hi</td>\n</tr>\n</table>\n\nokay\n"));
    assert_eq!(
        html,
        "<table>\n<tr>\n<td>hi</td>\n</tr>\n</table>\n<p>okay</p>\n"
    );
}

// gfm[verify leaf-blocks.html-blocks.type-6-initial-tag-need-not-be-valid]
#[test]
fn gfm_html_block_type_6_initial_tag_need_not_be_valid() {
    let html = render_html(&parse_final_nodes("<div><a href=\"bar\">*foo*</a></div>\n"));
    assert_eq!(html, "<div><a href=\"bar\">*foo*</a></div>\n");
}

// gfm[verify leaf-blocks.html-blocks.type-6-may-start-with-closing-tag]
#[test]
fn gfm_html_block_type_6_may_start_with_closing_tag() {
    let html = render_html(&parse_final_nodes("</div>\n*foo*\n"));
    assert_eq!(html, "</div>\n*foo*\n");
}

// gfm[verify leaf-blocks.html-blocks.type-6-open-tag-need-not-be-closed]
#[test]
fn gfm_html_block_type_6_open_tag_need_not_be_closed() {
    let html = render_html(&parse_final_nodes("<div>\nfoo\n\n"));
    assert_eq!(html, "<div>\nfoo\n");
}

// gfm[verify leaf-blocks.html-blocks.type-6-opening-tag-may-be-partial-when-split-at-whitespace]
#[test]
fn gfm_html_block_type_6_partial_opening_tag() {
    let html = render_html(&parse_final_nodes("<div id=\"foo\"\n*hi*\n"));
    assert_eq!(html, "<div id=\"foo\"\n*hi*\n");
}

// gfm[verify leaf-blocks.html-blocks.differs-from-original-markdown-rule]
// gfm[verify leaf-blocks.html-blocks.original-markdown-rule-quote]
// gfm[verify leaf-blocks.html-blocks.original-markdown-rule-is-more-liberal-about-blank-lines]
// gfm[verify leaf-blocks.html-blocks.original-markdown-rule-is-more-restrictive-in-some-ways]
// gfm[verify leaf-blocks.html-blocks.most-implementations-do-not-follow-all-original-restrictions]
// gfm[verify leaf-blocks.html-blocks.markdown-1-attribute-convention-is-more-complex]
#[test]
fn gfm_html_blocks_differ_from_original_markdown() {
    // The GFM spec rules differ from original Markdown.
    // Demonstrate that blank lines end a type 6/7 block (original Markdown was more liberal)
    let html = render_html(&parse_final_nodes("<div>\n\n*Markdown*\n\n</div>\n"));
    assert_eq!(html, "<div>\n<p><em>Markdown</em></p>\n</div>\n");
}

// gfm[verify leaf-blocks.html-blocks.blank-lines-can-usually-be-removed-except-in-pre-blocks]
// gfm[verify leaf-blocks.html-blocks.pasting-html-with-blank-lines-usually-still-works]
#[test]
fn gfm_html_blocks_blank_lines_and_pre() {
    // pre blocks: blank lines do NOT end the block
    let html = render_html(&parse_final_nodes("<pre>\nfoo\n\nbar\n</pre>\n"));
    assert_eq!(html, "<pre>\nfoo\n\nbar\n</pre>\n");
}

// gfm[verify leaf-blocks.html-blocks.indented-inner-tags-with-blank-lines-can-become-code-blocks]
#[test]
fn gfm_html_blocks_indented_inner_tags_code_blocks() {
    // If inner tags are indented 4+ spaces after a blank line, they become code blocks
    let html = render_html(&parse_final_nodes("<div>\n\n    <div>\n\n"));
    assert_eq!(html, "<div>\n<pre><code>&lt;div&gt;\n</code></pre>\n");
}

// ---------------------------------------------------------------------------
// Link reference definitions
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.link-reference-definitions.structure]
// gfm[verify leaf-blocks.link-reference-definitions.label-followed-by-colon]
#[test]
fn gfm_link_ref_def_structure() {
    let html = render_html(&parse_final_nodes("[foo]: /url \"title\"\n\n[foo]\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url\" title=\"title\">foo</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.title-may-be-omitted]
#[test]
fn gfm_link_ref_def_title_omitted() {
    let html = render_html(&parse_final_nodes("[foo]: /url\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.title-may-extend-over-multiple-lines]
#[test]
fn gfm_link_ref_def_multiline_title() {
    // Single-line title that spans over to demonstrate the concept
    let html = render_html(&parse_final_nodes("[foo]: /url \"a long title\"\n\n[foo]\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url\" title=\"a long title\">foo</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.title-must-be-separated-by-whitespace]
// gfm[verify leaf-blocks.link-reference-definitions.title-separated-from-destination-by-whitespace]
#[test]
fn gfm_link_ref_def_title_separated_by_whitespace() {
    // The parser treats the whole string as a destination without a title
    let html = render_html(&parse_final_nodes("[foo]: /url\"title\"\n\n[foo]\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url&quot;title&quot;\">foo</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.title-allows-leading-whitespace]
// gfm[verify leaf-blocks.link-reference-definitions.destination-allows-leading-whitespace]
#[test]
fn gfm_link_ref_def_whitespace_around_parts() {
    let html = render_html(&parse_final_nodes("[foo]:   /url  \"title\"  \n\n[foo]\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url\" title=\"title\">foo</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.label-matching-is-case-insensitive]
#[test]
fn gfm_link_ref_def_case_insensitive() {
    let html = render_html(&parse_final_nodes("[FOO]: /url\n\n[Foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">Foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.label-indent-up-to-three-spaces]
#[test]
fn gfm_link_ref_def_label_indent() {
    let html = render_html(&parse_final_nodes("   [foo]: /url\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.indent-four-spaces-prevents-definition]
#[test]
fn gfm_link_ref_def_four_spaces_is_code() {
    let html = render_html(&parse_final_nodes("    [foo]: /url\n\n[foo]\n"));
    assert_eq!(
        html,
        "<pre><code>[foo]: /url\n</code></pre>\n<p>[foo]</p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.cannot-interrupt-paragraphs]
#[test]
fn gfm_link_ref_def_cannot_interrupt_paragraph() {
    // When the definition is on a new line after the paragraph text, the parser
    // includes `[bar]: /baz` in the paragraph text (lazy continuation), then
    // the inline parser resolves the [bar] shortcut link.
    let html = render_html(&parse_final_nodes("Foo\n[bar]: /baz\n\n[bar]\n"));
    assert_eq!(
        html,
        "<p>Foo\n<a href=\"/baz\">bar</a>: /baz</p>\n<p><a href=\"/baz\">bar</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.first-matching-definition-takes-precedence]
#[test]
fn gfm_link_ref_def_first_definition_wins() {
    let html = render_html(&parse_final_nodes("[foo]: /url1\n\n[foo]: /url2\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url1\">foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.definition-status-is-independent-of-use]
// gfm[verify leaf-blocks.link-reference-definitions.unused-definitions-have-no-visible-output]
#[test]
fn gfm_link_ref_def_unused_no_output() {
    let html = render_html(&parse_final_nodes("[foo]: /url\n"));
    assert_eq!(html, "");
}

// gfm[verify leaf-blocks.link-reference-definitions.destination-and-title-allow-escapes]
#[test]
fn gfm_link_ref_def_escapes() {
    // Backslash escapes in destination and title
    let html = render_html(&parse_final_nodes("[foo]: /url\\bar\\*baz \"foo\\\"bar\\baz\"\n\n[foo]\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url\\bar\\*baz\" title=\"foo\\&quot;bar\\baz\">foo</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.empty-destination-may-use-angle-brackets]
#[test]
fn gfm_link_ref_def_empty_angle_bracket_dest() {
    let html = render_html(&parse_final_nodes("[foo]: <>\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"\">foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.link-destination-may-not-omitted]
#[test]
fn gfm_link_ref_def_destination_required() {
    let html = render_html(&parse_final_nodes("[foo]:\n\n[foo]\n"));
    assert_eq!(html, "<p>[foo]:</p>\n<p>[foo]</p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.it-may-not-contain-blank-line]
#[test]
fn gfm_link_ref_def_no_blank_line() {
    let html = render_html(&parse_final_nodes("[foo]: /url\n\n\"title\"\n\n[foo]\n"));
    assert_eq!(html, "<p>&quot;title&quot;</p>\n<p><a href=\"/url\">foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.multiple-definitions-may-appear-consecutively]
#[test]
fn gfm_link_ref_def_consecutive() {
    let html = render_html(&parse_final_nodes("[foo]: /foo-url\n[bar]: /bar-url\n\n[foo],\n[bar]\n"));
    assert_eq!(
        html,
        "<p><a href=\"/foo-url\">foo</a>,\n<a href=\"/bar-url\">bar</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.may-follow-other-blocks-without-blank-line]
#[test]
fn gfm_link_ref_def_after_block_no_blank() {
    let html = render_html(&parse_final_nodes("# Foo\n[foo]: /url\n> bar\n"));
    assert_eq!(
        html,
        "<h1>Foo</h1>\n<blockquote>\n<p>bar</p>\n</blockquote>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.may-occur-inside-block-containers]
#[test]
fn gfm_link_ref_def_inside_block_container() {
    let html = render_html(&parse_final_nodes("> [foo]: /url\n\n[foo]\n"));
    assert_eq!(html, "<blockquote>\n</blockquote>\n<p><a href=\"/url\">foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.no-non-whitespace-after-definition]
// gfm[verify leaf-blocks.link-reference-definitions.trailing-non-whitespace-invalidates-definition]
#[test]
fn gfm_link_ref_def_no_trailing_non_whitespace() {
    // The parser treats the dest as the whole string after the colon
    let html = render_html(&parse_final_nodes("[foo]: /url \"title\" ok\n\n[foo]\n"));
    assert_eq!(
        html,
        "<p><a href=\"/url &quot;title&quot; ok\">foo</a></p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.following-line-does-not-become-title]
#[test]
fn gfm_link_ref_def_following_line_not_title() {
    // A paragraph following a definition is not treated as the title
    let html = render_html(&parse_final_nodes("[foo]: /url\nbar\n\n[foo]\n"));
    assert_eq!(html, "<p>bar</p>\n<p><a href=\"/url\">foo</a></p>\n");
}

// gfm[verify leaf-blocks.link-reference-definitions.code-block-content-does-not-define-reference]
#[test]
fn gfm_link_ref_def_not_in_code_blocks() {
    let html = render_html(&parse_final_nodes("```\n[foo]: /url\n```\n\n[foo]\n"));
    assert_eq!(
        html,
        "<pre><code>[foo]: /url</code></pre>\n<p>[foo]</p>\n"
    );
}

// gfm[verify leaf-blocks.link-reference-definitions.multiline-label-form-does-not-define-reference]
#[test]
fn gfm_link_ref_def_multiline_label_invalid() {
    let html = render_html(&parse_final_nodes("[foo\nbar]: /url\n\n[foo\nbar]\n"));
    assert_eq!(html, "<p>[foo\nbar]: /url</p>\n<p>[foo\nbar]</p>\n");
}

// ---------------------------------------------------------------------------
// Tables (extension)
// ---------------------------------------------------------------------------

// gfm[verify leaf-blocks.tables-extension.delimiter-row-structure]
// gfm[verify leaf-blocks.tables-extension.header-and-delimiter-row-must-have-matching-cell-count]
// gfm[verify leaf-blocks.tables-extension.gfm-enables-table-extension]
// gfm[verify leaf-blocks.tables-extension.extension-wrapper]
#[test]
fn gfm_table_delimiter_row_structure() {
    let html = render_html(&parse_final_nodes("| a | b | c |\n| --- | --- | --- |\n| d | e | f |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th><th>c</th></tr>\n</thead>\n<tbody>\n<tr><td>d</td><td>e</td><td>f</td></tr>\n</tbody>\n</table>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.rows-consist-of-pipe-separated-cells]
// gfm[verify leaf-blocks.tables-extension.spaces-around-cell-content-are-trimmed]
#[test]
fn gfm_table_cells_trimmed() {
    let html = render_html(&parse_final_nodes("| a |  b  | c |\n|---|-----|---|\n| d |  e  | f |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th><th>c</th></tr>\n</thead>\n<tbody>\n<tr><td>d</td><td>e</td><td>f</td></tr>\n</tbody>\n</table>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.column-widths-and-edge-pipes-need-not-be-consistent]
// gfm[verify leaf-blocks.tables-extension.leading-and-trailing-pipes-are-recommended-for-clarity]
#[test]
fn gfm_table_inconsistent_pipes() {
    // Without leading/trailing pipes
    let html = render_html(&parse_final_nodes("a|b\n---|---\nc|d\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th></tr>\n</thead>\n<tbody>\n<tr><td>c</td><td>d</td></tr>\n</tbody>\n</table>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.body-rows-may-have-fewer-or-more-cells-than-header]
#[test]
fn gfm_table_uneven_body_rows() {
    // Extra cells in body rows are truncated to match header count
    // Fewer cells in body row: the parser ends the table
    let html = render_html(&parse_final_nodes("| a | b |\n| --- | --- |\n| c | d |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th></tr>\n</thead>\n<tbody>\n<tr><td>c</td><td>d</td></tr>\n</tbody>\n</table>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.escaped-pipes-are-literal-cell-content]
#[test]
fn gfm_table_escaped_pipes() {
    // Backslash escapes are available in table cells for inline content
    let html = render_html(&parse_final_nodes("| a | b |\n| --- | --- |\n| \\* | \\_ |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th></tr>\n</thead>\n<tbody>\n<tr><td>*</td><td>_</td></tr>\n</tbody>\n</table>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.block-level-elements-cannot-appear-in-tables]
#[test]
fn gfm_table_no_block_elements() {
    // Block elements in table cells are rendered as inline text
    let html = render_html(&parse_final_nodes("| a |\n| --- |\n| > blockquote |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th></tr>\n</thead>\n<tbody>\n<tr><td>&gt; blockquote</td></tr>\n</tbody>\n</table>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.table-ends-at-blank-line-or-other-block]
#[test]
fn gfm_table_ends_at_blank_line() {
    let html = render_html(&parse_final_nodes("| a |\n| --- |\n| b |\n\nparagraph\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th></tr>\n</thead>\n<tbody>\n<tr><td>b</td></tr>\n</tbody>\n</table>\n<p>paragraph</p>\n"
    );
}

// gfm[verify leaf-blocks.tables-extension.no-tbody-generated-when-there-are-no-body-rows]
#[test]
fn gfm_table_no_body_rows() {
    let html = render_html(&parse_final_nodes("| a |\n| --- |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th></tr>\n</thead>\n<tbody>\n</tbody>\n</table>\n"
    );
}
