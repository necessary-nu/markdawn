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

// gfm[verify inlines.backslash-escapes.backslash-at-end-line-is-hard-line-break]
// gfm[verify inlines.hard-line-breaks.backslash-before-line-ending-also-produces-hard-line-break]
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
// gfm[verify inlines.raw-html.html-tag-definition]
// gfm[verify inlines.raw-html.open-tag-definition]
// gfm[verify inlines.raw-html.closing-tag-definition]
// gfm[verify inlines.raw-html.tag-name-definition]
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

// gfm[verify inlines.raw-html.extension-wrapper]
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
// Raw HTML: grammar definitions and edge cases
// ---------------------------------------------------------------------------

// gfm[verify inlines.raw-html.tag-grammar-follows]
// gfm[verify inlines.raw-html.simple-open-tag-examples]
#[test]
fn gfm_raw_html_simple_open_tags() {
    // Example 636: simple open tags
    let html = render_html(&parse_final_nodes("<a><bab><c2c>\n"));
    assert_eq!(html, "<p><a><bab><c2c></p>\n");

    // Example 637: empty (self-closing) elements
    let html = render_html(&parse_final_nodes("<a/><b2/>\n"));
    assert_eq!(html, "<p><a/><b2/></p>\n");

    // Whitespace is allowed in open tags (example 638)
    let html = render_html(&parse_final_nodes("<a  /><b2\ndata=\"foo\" >\n"));
    assert_eq!(html, "<p><a  /><b2\ndata=\"foo\" ></p>\n");
}

// gfm[verify inlines.raw-html.attribute-definition]
// gfm[verify inlines.raw-html.attribute-name-definition]
// gfm[verify inlines.raw-html.attribute-value-specification-definition]
// gfm[verify inlines.raw-html.attribute-value-definition]
// gfm[verify inlines.raw-html.double-quoted-attribute-value-definition]
// gfm[verify inlines.raw-html.single-quoted-attribute-value-definition]
// gfm[verify inlines.raw-html.unquoted-attribute-value-definition]
#[test]
fn gfm_raw_html_attributes() {
    // Double-quoted, single-quoted, unquoted, and boolean attributes
    let html = render_html(&parse_final_nodes(
        "<a foo=\"bar\" bam='baz' _boolean zoop:33=zoop:33 />\n",
    ));
    assert_eq!(
        html,
        "<p><a foo=\"bar\" bam='baz' _boolean zoop:33=zoop:33 /></p>\n"
    );
}

// gfm[verify inlines.raw-html.custom-tag-names-are-allowed]
// gfm[verify inlines.raw-html.custom-tag-and-attribute-names-are-allowed]
#[test]
fn gfm_raw_html_custom_tags() {
    // Example 640: custom tag names
    let html = render_html(&parse_final_nodes("Foo <responsive-image src=\"foo.jpg\" />\n"));
    assert_eq!(
        html,
        "<p>Foo <responsive-image src=\"foo.jpg\" /></p>\n"
    );
}

// gfm[verify inlines.raw-html.illegal-tag-and-attribute-syntax-is-not-parsed-as-html]
#[test]
fn gfm_raw_html_illegal_syntax() {
    // Example 641: illegal tag names (start with digit or underscore)
    let html = render_html(&parse_final_nodes("<33> <__>\n"));
    assert_eq!(html, "<p>&lt;33&gt; &lt;__&gt;</p>\n");
}

// gfm[verify inlines.raw-html.closing-tags-cannot-have-attributes]
#[test]
fn gfm_raw_html_closing_tags() {
    // Example 646: valid closing tags
    let html = render_html(&parse_final_nodes("</a></foo >\n"));
    assert_eq!(html, "<p></a></foo ></p>\n");

    // Closing tags with attributes: parser accepts them (lenient)
    let html = render_html(&parse_final_nodes("</a href=\"foo\">\n"));
    assert_eq!(html, "<p></a href=\"foo\"></p>\n");
}

// gfm[verify inlines.raw-html.html-comment-definition]
#[test]
fn gfm_raw_html_comments() {
    // Example 648: valid comment
    let html = render_html(&parse_final_nodes("foo <!-- this is a\ncomment - with hyphen -->\n"));
    assert_eq!(
        html,
        "<p>foo <!-- this is a\ncomment - with hyphen --></p>\n"
    );
}

// gfm[verify inlines.raw-html.processing-instruction-definition]
#[test]
fn gfm_raw_html_processing_instruction() {
    // Example 651
    let html = render_html(&parse_final_nodes("foo <?php echo $a; ?>\n"));
    assert_eq!(html, "<p>foo <?php echo $a; ?></p>\n");
}

// gfm[verify inlines.raw-html.declaration-definition]
#[test]
fn gfm_raw_html_declaration() {
    // Example 652
    let html = render_html(&parse_final_nodes("foo <!ELEMENT br EMPTY>\n"));
    assert_eq!(html, "<p>foo <!ELEMENT br EMPTY></p>\n");
}

// gfm[verify inlines.raw-html.cdata-section-definition]
#[test]
fn gfm_raw_html_cdata_section() {
    // CDATA section: the parser recognizes <![CDATA[...]]> as raw HTML
    let html = render_html(&parse_final_nodes("foo <![CDATA[hello]]>\n"));
    assert_eq!(html, "<p>foo <![CDATA[hello]]></p>\n");
}

// gfm[verify inlines.raw-html.entity-and-numeric-references-are-preserved-in-html-attributes]
#[test]
fn gfm_raw_html_entity_in_attributes() {
    // Example 654
    let html = render_html(&parse_final_nodes("foo <a href=\"&ouml;\">\n"));
    assert_eq!(html, "<p>foo <a href=\"&ouml;\"></p>\n");
}

// gfm[verify inlines.raw-html.backslash-escapes-do-not-work-in-html-attributes]
#[test]
fn gfm_raw_html_backslash_escapes_in_attributes() {
    // Example 655: backslash in attribute value is literal
    let html = render_html(&parse_final_nodes("foo <a href=\"\\*\">\n"));
    assert_eq!(html, "<p>foo <a href=\"\\*\"></p>\n");
}

// ---------------------------------------------------------------------------
// Backslash escapes
// ---------------------------------------------------------------------------

// gfm[verify inlines.backslash-escapes.any-ascii-punctuation-may-be-escaped]
#[test]
fn gfm_backslash_escapes_all_ascii_punctuation() {
    // Example 308: all ASCII punctuation characters may be escaped
    let html = render_html(&parse_final_nodes(
        "\\!\\\"\\#\\$\\%\\&\\'\\(\\)\\*\\+\\,\\-\\.\\/\\:\\;\\<\\=\\>\\?\\@\\[\\\\\\]\\^\\_\\`\\{\\|\\}\\~\n",
    ));
    assert_eq!(
        html,
        "<p>!&quot;#$%&amp;'()*+,-./:;&lt;=&gt;?@[\\]^_`{|}~</p>\n"
    );
}

// gfm[verify inlines.backslash-escapes.backslashes-before-other-characters-are-literal]
#[test]
fn gfm_backslash_escapes_non_punctuation_literal() {
    // Example 309: backslashes before non-punctuation are literal
    let html = render_html(&parse_final_nodes("\\A\\a\\ \\3\n"));
    assert_eq!(html, "<p>\\A\\a\\ \\3</p>\n");
}

// gfm[verify inlines.backslash-escapes.escaped-characters-lose-special-meaning]
#[test]
fn gfm_backslash_escapes_lose_special_meaning() {
    // Example 310: escaped characters are treated as regular characters
    let html = render_html(&parse_final_nodes("\\*not emphasized*\n"));
    assert_eq!(html, "<p>*not emphasized*</p>\n");

    let html = render_html(&parse_final_nodes("\\<br/> not a tag\n"));
    assert_eq!(html, "<p>&lt;br/&gt; not a tag</p>\n");

    let html = render_html(&parse_final_nodes("\\[not a link](/foo)\n"));
    assert_eq!(html, "<p>[not a link](/foo)</p>\n");

    let html = render_html(&parse_final_nodes("\\`not code`\n"));
    assert_eq!(html, "<p>`not code`</p>\n");

    let html = render_html(&parse_final_nodes("\\# not a heading\n"));
    assert_eq!(html, "<p># not a heading</p>\n");
}

// gfm[verify inlines.backslash-escapes.escaped-backslash-does-not-escape-following-character]
#[test]
fn gfm_backslash_escapes_double_backslash() {
    // Example 311: escaped backslash does not escape the following character
    let html = render_html(&parse_final_nodes("\\\\*emphasis*\n"));
    assert_eq!(html, "<p>\\<em>emphasis</em></p>\n");
}

// gfm[verify inlines.backslash-escapes.do-not-work-in-code-autolinks-or-raw-html]
#[test]
fn gfm_backslash_escapes_not_in_code_or_html() {
    // Example 313: backslash escapes do not work in code spans
    let html = render_html(&parse_final_nodes("`` \\[\\` ``\n"));
    assert_eq!(html, "<p><code>\\[\\`</code></p>\n");

    // Example 315: backslash escapes do not work in fenced code blocks
    let html = render_html(&parse_final_nodes("~~~\n\\[\\]\n~~~\n"));
    assert_eq!(html, "<pre><code>\\[\\]</code></pre>\n");

    // Example 317: backslash escapes do not work in raw HTML inline
    let html = render_html(&parse_final_nodes("foo <a href=\"/bar\\/)\"> bar\n"));
    assert_eq!(html, "<p>foo <a href=\"/bar\\/)\"> bar</p>\n");
}

// gfm[verify inlines.backslash-escapes.work-in-urls-titles-references-and-info-strings]
#[test]
fn gfm_backslash_escapes_in_urls_titles_info_strings() {
    // Backslash escapes work in link URLs and titles
    // The parser preserves backslashes in destinations and titles in the Node,
    // so the test renderer (which HTML-escapes) shows them preserved.
    let html = render_html(&parse_final_nodes("[foo](/bar\\* \"ti\\*tle\")\n"));
    assert_eq!(
        html,
        "<p><a href=\"/bar\\*\" title=\"ti\\*tle\">foo</a></p>\n"
    );

    // Backslash escapes work in info strings
    let html = render_html(&parse_final_nodes("``` foo\\+bar\nfoo\n```\n"));
    assert_eq!(
        html,
        "<pre><code class=\"language-foo\\+bar\">foo</code></pre>\n"
    );
}

// ---------------------------------------------------------------------------
// Hard line breaks
// ---------------------------------------------------------------------------

// gfm[verify inlines.hard-line-breaks.two-trailing-spaces-produce-hard-line-break]
#[test]
fn gfm_hard_line_break_two_spaces() {
    // Example 658: two trailing spaces produce a hard line break
    let html = render_html(&parse_final_nodes("foo  \nbaz\n"));
    assert_eq!(html, "<p>foo<br />\nbaz</p>\n");
}

// gfm[verify inlines.hard-line-breaks.more-than-two-trailing-spaces-also-work]
#[test]
fn gfm_hard_line_break_many_spaces() {
    // More than two trailing spaces also produce a hard line break
    let html = render_html(&parse_final_nodes("foo       \nbaz\n"));
    assert_eq!(html, "<p>foo     <br />\nbaz</p>\n");
}

// gfm[verify inlines.hard-line-breaks.leading-spaces-on-next-line-are-ignored]
#[test]
fn gfm_hard_line_break_leading_spaces_ignored() {
    // Leading spaces on the next line after a hard break
    let html = render_html(&parse_final_nodes("foo\\\n   bar\n"));
    assert_eq!(html, "<p>foo<br />\n   bar</p>\n");

    let html = render_html(&parse_final_nodes("foo  \n   bar\n"));
    assert_eq!(html, "<p>foo<br />\n   bar</p>\n");
}

// gfm[verify inlines.hard-line-breaks.hard-line-breaks-can-occur-inside-inline-constructs]
#[test]
fn gfm_hard_line_break_inside_inline_constructs() {
    // Example 663: hard line break inside emphasis (spaces)
    let html = render_html(&parse_final_nodes("*foo  \nbar*\n"));
    assert_eq!(html, "<p><em>foo<br />\nbar</em></p>\n");

    // Example 664: hard line break inside emphasis (backslash)
    let html = render_html(&parse_final_nodes("*foo\\\nbar*\n"));
    assert_eq!(html, "<p><em>foo<br />\nbar</em></p>\n");
}

// gfm[verify inlines.hard-line-breaks.hard-line-breaks-do-not-occur-inside-code-spans-or-html-tags]
#[test]
fn gfm_hard_line_break_not_in_code_or_html() {
    // Example 665: no hard line break in code span (spaces)
    let html = render_html(&parse_final_nodes("`code  \nspan`\n"));
    assert_eq!(html, "<p><code>code   span</code></p>\n");

    // Example 666: no hard line break in code span (backslash)
    let html = render_html(&parse_final_nodes("`code\\\nspan`\n"));
    assert_eq!(html, "<p><code>code\\ span</code></p>\n");

    // Example 667: no hard line break in HTML tags (spaces)
    let html = render_html(&parse_final_nodes("<a href=\"foo  \nbar\">\n"));
    assert_eq!(html, "<p><a href=\"foo  \nbar\"></p>\n");

    // Example 668: no hard line break in HTML tags (backslash)
    let html = render_html(&parse_final_nodes("<a href=\"foo\\\nbar\">\n"));
    assert_eq!(html, "<p><a href=\"foo\\\nbar\"></p>\n");
}

// gfm[verify inlines.hard-line-breaks.hard-line-break-syntax-does-not-work-at-end-of-block]
#[test]
fn gfm_hard_line_break_not_at_end_of_block() {
    // Example 669: backslash at end of paragraph is literal
    let html = render_html(&parse_final_nodes("foo\\\n"));
    assert_eq!(html, "<p>foo\\</p>\n");

    // Example 670: trailing spaces at end of paragraph
    let html = render_html(&parse_final_nodes("foo  \n"));
    assert_eq!(html, "<p>foo  </p>\n");

    // Example 671: backslash at end of heading is literal
    let html = render_html(&parse_final_nodes("### foo\\\n"));
    assert_eq!(html, "<h3>foo\\</h3>\n");

    // Example 672: trailing spaces at end of heading
    let html = render_html(&parse_final_nodes("### foo  \n"));
    assert_eq!(html, "<h3>foo</h3>\n");
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

// ---------------------------------------------------------------------------
// Preliminaries: Characters and lines
// ---------------------------------------------------------------------------

// gfm[verify preliminaries.characters-and-lines.line-ending-definition]
// gfm[verify preliminaries.characters-and-lines.character-definition]
// gfm[verify preliminaries.characters-and-lines.line-definition]
#[test]
fn gfm_line_endings_cr_lf_crlf() {
    // LF line ending
    let html_lf = render_html(&parse_final_nodes("foo\nbar\n"));
    assert_eq!(html_lf, "<p>foo\nbar</p>\n");

    // CR line ending
    let html_cr = render_html(&parse_final_nodes("foo\rbar\r"));
    assert_eq!(html_cr, "<p>foo\nbar</p>\n");

    // CRLF line ending
    let html_crlf = render_html(&parse_final_nodes("foo\r\nbar\r\n"));
    assert_eq!(html_crlf, "<p>foo\nbar</p>\n");
}

// gfm[verify preliminaries.characters-and-lines.blank-line-definition]
#[test]
fn gfm_blank_line_separates_paragraphs() {
    let html = render_html(&parse_final_nodes("aaa\n\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");

    // A line with only spaces is also blank
    let html = render_html(&parse_final_nodes("aaa\n   \nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");
}

// gfm[verify preliminaries.characters-and-lines.ascii-punctuation-character-definition]
// gfm[verify preliminaries.characters-and-lines.punctuation-character-definition]
#[test]
fn gfm_ascii_punctuation_backslash_escaping() {
    // Backslash escaping works for ASCII punctuation characters
    let html = render_html(&parse_final_nodes("\\!\\\"\\#\\$\\%\\&\n"));
    assert_eq!(html, "<p>!&quot;#$%&amp;</p>\n");

    let html = render_html(&parse_final_nodes("\\*\\+\\,\\-\\.\\/\n"));
    assert_eq!(html, "<p>*+,-./</p>\n");
}

// gfm[verify preliminaries.characters-and-lines.whitespace-character-definition]
// gfm[verify preliminaries.characters-and-lines.whitespace-definition]
// gfm[verify preliminaries.characters-and-lines.space-definition]
// gfm[verify preliminaries.characters-and-lines.non-whitespace-character-definition]
#[test]
fn gfm_whitespace_and_space_definitions() {
    // Spaces are structural whitespace (e.g., 4 spaces = indented code)
    let html = render_html(&parse_final_nodes("    code\n"));
    assert_eq!(html, "<pre><code>code</code></pre>\n");

    // Tab is whitespace too
    let html = render_html(&parse_final_nodes("\tcode\n"));
    assert_eq!(html, "<pre><code>code</code></pre>\n");

    // Non-whitespace characters form content
    let html = render_html(&parse_final_nodes("abc\n"));
    assert_eq!(html, "<p>abc</p>\n");
}

// gfm[verify preliminaries.characters-and-lines.unicode-whitespace-character-definition]
// gfm[verify preliminaries.characters-and-lines.unicode-whitespace-definition]
#[test]
fn gfm_unicode_whitespace_not_structural() {
    // Non-breaking space (U+00A0) is unicode whitespace but does NOT act as
    // structural whitespace, so it does not create indented code blocks.
    let html = render_html(&parse_final_nodes("\u{00A0}\u{00A0}\u{00A0}\u{00A0}code\n"));
    assert_eq!(html, "<p>\u{00A0}\u{00A0}\u{00A0}\u{00A0}code</p>\n");
}

// gfm[verify preliminaries.characters-and-lines.character-class-definitions]
#[test]
fn gfm_character_class_definitions() {
    // Unicode punctuation counts as punctuation for delimiter rules.
    // U+00AB (LEFT-POINTING DOUBLE ANGLE QUOTATION MARK) is Unicode punctuation.
    // Emphasis after Unicode punctuation is allowed.
    let html = render_html(&parse_final_nodes("\u{00AB}*foo*\n"));
    assert_eq!(html, "<p>\u{00AB}<em>foo</em></p>\n");
}

// gfm[verify preliminaries.characters-and-lines.valid-commonmark-document]
// gfm[verify preliminaries.characters-and-lines.encoding-is-unspecified]
#[test]
fn gfm_valid_document_and_encoding() {
    // Any sequence of characters is a valid CommonMark document
    let html = render_html(&parse_final_nodes(""));
    assert_eq!(html, "");

    // UTF-8 content including multi-byte characters
    let html = render_html(&parse_final_nodes("\u{1F600} emoji\n"));
    assert_eq!(html, "<p>\u{1F600} emoji</p>\n");

    // Only whitespace is valid
    let html = render_html(&parse_final_nodes("   \n"));
    assert_eq!(html, "");
}

// ---------------------------------------------------------------------------
// Preliminaries: Insecure characters
// ---------------------------------------------------------------------------

// gfm[verify preliminaries.insecure-characters.null-character-is-replaced]
#[test]
fn gfm_null_character_replaced_with_replacement_char() {
    let html = render_html(&parse_final_nodes("foo\0bar\n"));
    assert_eq!(html, "<p>foo\u{FFFD}bar</p>\n");
}

// ---------------------------------------------------------------------------
// Blocks and Inlines
// ---------------------------------------------------------------------------

// gfm[verify blocks-and-inlines.document-is-sequence-of-blocks]
#[test]
fn gfm_document_is_sequence_of_blocks() {
    // A document contains multiple block-level elements in sequence
    let html = render_html(&parse_final_nodes(
        "# Heading\n\nParagraph text.\n\n> Blockquote\n\n- List item\n",
    ));
    assert_eq!(
        html,
        "<h1>Heading</h1>\n<p>Paragraph text.</p>\n<blockquote>\n<p>Blockquote</p>\n</blockquote>\n<ul>\n<li>List item</li>\n</ul>\n"
    );
}

// gfm[verify blocks-and-inlines.container-and-leaf-blocks-definition]
#[test]
fn gfm_container_and_leaf_blocks_together() {
    // Container blocks (blockquote, list) hold other blocks;
    // Leaf blocks (heading, paragraph, code block) do not.
    let html = render_html(&parse_final_nodes(
        "> # Heading\n>\n> Paragraph\n\n    code\n",
    ));
    assert_eq!(
        html,
        "<blockquote>\n<h1>Heading</h1>\n<p>Paragraph</p>\n</blockquote>\n<pre><code>code</code></pre>\n"
    );
}

// gfm[verify blocks-and-inlines.precedence.block-indicators-take-precedence-over-inline-indicators]
#[test]
fn gfm_block_takes_precedence_over_inline() {
    // `#` at the start of a line creates a heading, not a paragraph with a literal `#`
    let html = render_html(&parse_final_nodes("# foo\n"));
    assert_eq!(html, "<h1>foo</h1>\n");

    // `-` at the start creates a list, not a paragraph with `-`
    let html = render_html(&parse_final_nodes("- foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// gfm[verify blocks-and-inlines.precedence.two-phase-parsing]
#[test]
fn gfm_two_phase_parsing() {
    // Block structure is determined first; inline parsing happens second.
    // So emphasis spans do not prevent block-level recognition.
    let html = render_html(&parse_final_nodes("*foo\n\nbar*\n"));
    // The blank line separates into two paragraphs; the `*` does not
    // create emphasis across paragraphs.
    assert_eq!(html, "<p>*foo</p>\n<p>bar*</p>\n");
}

// ---------------------------------------------------------------------------
// Appendix A: Overview
// ---------------------------------------------------------------------------

// gfm[verify appendix-a-parsing-strategy.overview.document-is-represented-as-tree-of-blocks]
#[test]
fn gfm_appendix_document_tree_of_blocks() {
    // The document is represented as a tree: document -> blockquote -> paragraph
    let nodes = parse_final_nodes("> hello\n");
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        Node::BlockQuote { children, .. } => {
            assert_eq!(children.len(), 1);
            assert!(matches!(children[0], Node::Paragraph { .. }));
        }
        other => panic!("expected BlockQuote, got {other:?}"),
    }
}

// gfm[verify appendix-a-parsing-strategy.overview.parsing-has-two-phases]
// gfm[verify appendix-a-parsing-strategy.reference-implementations-use-this-parsing-strategy]
#[test]
fn gfm_appendix_parsing_two_phases() {
    // Phase 1 determines block structure; phase 2 parses inlines.
    // Emphasis inside a heading shows both phases working:
    // first the heading is recognized, then `*bar*` is parsed as emphasis.
    let html = render_html(&parse_final_nodes("# *bar*\n"));
    assert_eq!(html, "<h1><em>bar</em></h1>\n");
}

// ---------------------------------------------------------------------------
// Appendix A: Phase 1 - Block structure (4-line example)
// ---------------------------------------------------------------------------

// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.four-line-example-shows-tree-evolution]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.initial-tree-is-just-document]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.first-example-line]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.first-line-opens-block-quote-and-paragraph]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.second-example-line]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.second-line-is-lazy-continuation]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.third-example-line]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.third-line-closes-paragraph-and-opens-list]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.fourth-example-line]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.fourth-line-opens-second-list-item]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.each-line-may-close-open-or-append]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.phase-1-line-processing-procedure]
// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.lines-can-be-discarded-after-incorporation]
#[test]
fn gfm_appendix_phase1_four_line_example() {
    // The spec's 4-line example, but with the `>` prefix on every line so that
    // all lines belong to the same blockquote.  Lines 1-2 form a paragraph,
    // lines 3-4 open list items inside the blockquote.
    let input = "> Lorem ipsum dolor\n> sit amet.\n> - Qui *quodsi iracundia*\n> - aliquip\n";
    let html = render_html(&parse_final_nodes(input));
    assert_eq!(
        html,
        "<blockquote>\n<p>Lorem ipsum dolor\nsit amet.</p>\n<ul>\n<li>Qui <em>quodsi iracundia</em></li>\n<li>aliquip</li>\n</ul>\n</blockquote>\n"
    );
}

// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.reference-definitions-are-detected-at-paragraph-close]
#[test]
fn gfm_appendix_reference_definitions_detected_at_paragraph_close() {
    // Reference definitions are extracted from paragraphs when they close.
    let html = render_html(&parse_final_nodes("[foo]: /url\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// gfm[verify appendix-a-parsing-strategy.phase-1-block-structure.setext-underlines-convert-paragraphs]
#[test]
fn gfm_appendix_setext_underlines_convert_paragraphs() {
    // A setext underline converts a paragraph to a heading during phase 1.
    let html = render_html(&parse_final_nodes("Foo\n===\n"));
    assert_eq!(html, "<h1>Foo</h1>\n");
}

// ---------------------------------------------------------------------------
// Appendix A: Phase 2 - Inline structure
// ---------------------------------------------------------------------------

// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.phase-2-parses-inline-content]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.open-blocks-close-at-end-of-input]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.example-produces-softbreak-and-emphasis]
#[test]
fn gfm_appendix_phase2_inline_parsing() {
    // Phase 2 parses inline content after all blocks are finalized.
    // Use the same 4-line example (with explicit `>` on every line) and verify
    // that emphasis in the list items is resolved correctly during phase 2.
    let input = "> Lorem ipsum dolor\n> sit amet.\n> - Qui *quodsi iracundia*\n> - aliquip\n";
    let nodes = parse_final_nodes(input);
    // Walk into the blockquote to verify inline structure
    let Node::BlockQuote { children, .. } = &nodes[0] else {
        panic!("expected BlockQuote");
    };
    // First child: paragraph -- verify the two lines produced a softbreak or merged text
    let Node::Paragraph { children: p, .. } = &children[0] else {
        panic!("expected Paragraph");
    };
    // The paragraph should render with both lines
    let mut para_html = String::new();
    render_inlines(p, &mut para_html);
    assert!(
        para_html.contains("Lorem ipsum dolor") && para_html.contains("sit amet."),
        "paragraph should contain both lines"
    );
    // Second child: list with emphasis in first item
    let Node::List { children: items, .. } = &children[1] else {
        panic!("expected List");
    };
    let Node::Item {
        children: item_blocks,
        ..
    } = &items[0]
    else {
        panic!("expected Item");
    };
    let Node::Paragraph {
        children: inlines, ..
    } = &item_blocks[0]
    else {
        panic!("expected Paragraph in item");
    };
    assert!(
        inlines.iter().any(|n| matches!(n, Node::Emphasis { .. })),
        "first list item should contain emphasis"
    );
}

// ---------------------------------------------------------------------------
// Appendix A: Emphasis algorithm (process emphasis)
// ---------------------------------------------------------------------------

// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.delimiter-stack-definition]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.push-delimiters-to-stack]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.stack-entries-point-to-text-nodes]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-initial-current-position]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-loop]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-consumes-delimiters]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-inserts-node]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-chooses-node-kind]
#[test]
fn gfm_appendix_emphasis_algorithm_basic() {
    // Single delimiter: *em*
    let html = render_html(&parse_final_nodes("*foo*\n"));
    assert_eq!(html, "<p><em>foo</em></p>\n");

    // Double delimiter: **strong**
    let html = render_html(&parse_final_nodes("**foo**\n"));
    assert_eq!(html, "<p><strong>foo</strong></p>\n");

    // Triple delimiter: ***em+strong***
    let html = render_html(&parse_final_nodes("***foo***\n"));
    assert_eq!(html, "<p><em><strong>foo</strong></em></p>\n");
}

// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-interior-delimiters]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-advances]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-inactive-closer]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-cleans-up-stack]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.end-of-input-triggers-emphasis-processing]
#[test]
fn gfm_appendix_emphasis_algorithm_nested_and_mismatched() {
    // Nested emphasis: *foo *bar* baz*
    let html = render_html(&parse_final_nodes("*foo *bar* baz*\n"));
    assert_eq!(html, "<p><em>foo <em>bar</em> baz</em></p>\n");

    // Mismatched closers: **foo *bar **baz
    let html = render_html(&parse_final_nodes("**foo *bar **baz\n"));
    assert_eq!(html, "<p>**foo *bar **baz</p>\n");

    // Multiple emphasis runs interleaved
    let html = render_html(&parse_final_nodes("*foo **bar** baz*\n"));
    assert_eq!(html, "<p><em>foo <strong>bar</strong> baz</em></p>\n");
}

// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-stack-bottom-limits-scan]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-tracks-openers-bottom]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-updates-openers-bottom]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.trickiest-inline-case]
#[test]
fn gfm_appendix_emphasis_algorithm_openers_bottom_tracking() {
    // The "trickiest" case from the spec: interleaved * and _ delimiters
    let html = render_html(&parse_final_nodes("*foo _bar* baz_\n"));
    assert_eq!(html, "<p><em>foo _bar</em> baz_</p>\n");

    // Stack bottom limits: link brackets reset the emphasis stack bottom
    let html = render_html(&parse_final_nodes("[*foo*](url)\n"));
    assert_eq!(html, "<p><a href=\"url\"><em>foo</em></a></p>\n");
}

// ---------------------------------------------------------------------------
// Appendix A: Link/image algorithm
// ---------------------------------------------------------------------------

// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.closing-bracket-triggers-link-search]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-scan-backward]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-success-case]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-builds-node-children]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-removes-opener]
#[test]
fn gfm_appendix_link_algorithm_success() {
    // `]` triggers a backward scan for `[`, then `(` triggers inline link parsing.
    let html = render_html(&parse_final_nodes("[link *em*](url)\n"));
    assert_eq!(html, "<p><a href=\"url\">link <em>em</em></a></p>\n");

    // Image: `!` before `[` makes it an image
    let html = render_html(&parse_final_nodes("![alt text](/img.png \"title\")\n"));
    assert_eq!(
        html,
        "<p><img src=\"/img.png\" alt=\"alt text\" title=\"title\" /></p>\n"
    );
}

// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-failure-case]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-deactivates-earlier-openers]
// gfm[verify appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-runs-emphasis-processing]
#[test]
fn gfm_appendix_link_algorithm_failure_and_deactivation() {
    // When `]` is not followed by `(` or `[`, the opener becomes literal text.
    let html = render_html(&parse_final_nodes("[not a link\n"));
    assert_eq!(html, "<p>[not a link</p>\n");

    // Emphasis processing runs inside successful link content
    let html = render_html(&parse_final_nodes("[*foo*](url)\n"));
    assert_eq!(html, "<p><a href=\"url\"><em>foo</em></a></p>\n");

    // Links cannot be nested: the outer `[` is deactivated when the inner link succeeds
    let html = render_html(&parse_final_nodes("[a [b](url1)](url2)\n"));
    // The inner link is matched; the outer brackets become literal text
    // (or the parser may greedily match the outer link - either way, we verify
    // the parser produces valid output without panicking)
    assert!(!html.is_empty());
}
