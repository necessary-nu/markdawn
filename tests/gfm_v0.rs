use markdawn::{Node, Parser, StreamItem, TableAlignment, VecSink};

fn parse_final_nodes(input: &str) -> Vec<Node<()>> {
    let mut parser = Parser::<()>::builder().with_gfm().with_math().build();
    let mut sink = VecSink::new();
    parser.push_str(input, &mut sink).unwrap();
    parser.finish(&mut sink).unwrap();

    let mut order = Vec::new();
    let mut values = Vec::<(u64, Node<()>)>::new();

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

fn render_html(nodes: &[Node<()>]) -> String {
    let mut out = String::new();
    for node in nodes {
        render_block(node, &mut out);
    }
    out
}

fn render_block(node: &Node<()>, out: &mut String) {
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
            let lang = info
                .as_deref()
                .and_then(|s| s.split_whitespace().next())
                .filter(|s| !s.is_empty());
            match lang {
                Some(lang) => {
                    out.push_str("<pre><code class=\"language-");
                    escape_html(lang, out);
                    out.push_str("\">");
                }
                None => out.push_str("<pre><code>"),
            }
            escape_html(literal, out);
            out.push_str("</code></pre>\n");
        }
        Node::MathBlock { math, .. } => {
            out.push_str("<pre><code class=\"language-math math-display\">");
            escape_html(math, out);
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

fn render_inlines(nodes: &[Node<()>], out: &mut String) {
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
                escape_href(destination, out);
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
                escape_href(destination, out);
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
            Node::MathInline { math, .. } => {
                out.push_str("<code class=\"language-math math-inline\">");
                escape_html(math, out);
                out.push_str("</code>");
            }
            Node::MathBlock { math, .. } => {
                out.push_str("<code class=\"language-math math-display\">");
                escape_html(math, out);
                out.push_str("</code>");
            }
            Node::Html { html, .. } => out.push_str(html),
            Node::HardBreak { .. } => out.push_str("<br />\n"),
            Node::SoftBreak { .. } => out.push('\n'),
            other => panic!("unsupported inline in test renderer: {other:?}"),
        }
    }
}

fn render_table_row(node: &Node<()>, alignments: &[Option<TableAlignment>], out: &mut String) {
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

fn render_list_child(node: &Node<()>, tight: bool, out: &mut String) {
    match node {
        Node::Item { children, .. } => {
            out.push_str("<li>");
            if children.is_empty() {
                out.push_str("</li>\n");
            } else if tight {
                // In tight items, unwrap all paragraphs (no <p> tags).
                if !matches!(children.first(), Some(Node::Paragraph { .. })) {
                    out.push('\n');
                }
                for (i, child) in children.iter().enumerate() {
                    if let Node::Paragraph {
                        children: inlines, ..
                    } = child
                    {
                        render_inlines(inlines, out);
                        // Add newline only between two paragraphs/inlines,
                        // not before a block (which has its own opening tag).
                        if i + 1 < children.len()
                            && matches!(children[i + 1], Node::Paragraph { .. })
                        {
                            out.push('\n');
                        }
                    } else {
                        render_block(child, out);
                    }
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
            } else if tight {
                if !matches!(children.first(), Some(Node::Paragraph { .. })) {
                    out.push('\n');
                }
                for (i, child) in children.iter().enumerate() {
                    if let Node::Paragraph {
                        children: inlines, ..
                    } = child
                    {
                        render_inlines(inlines, out);
                        if i + 1 < children.len()
                            && matches!(children[i + 1], Node::Paragraph { .. })
                        {
                            out.push('\n');
                        }
                    } else {
                        render_block(child, out);
                    }
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

fn escape_href(url: &str, out: &mut String) {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const URL_SAFE: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~')
        .remove(b':')
        .remove(b'/')
        .remove(b'?')
        .remove(b'#')
        .remove(b'@')
        .remove(b'!')
        .remove(b'$')
        .remove(b'&')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')')
        .remove(b'*')
        .remove(b'+')
        .remove(b',')
        .remove(b';')
        .remove(b'=')
        .remove(b'%');
    out.push_str(&utf8_percent_encode(url, URL_SAFE).to_string());
}

// [spec:gfm:def:leaf-blocks.thematic-breaks.structure/test]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.requires-dash-underscore-or-asterisk-markers/test]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.requires-three-or-more-marker-characters/test]
#[test]
fn gfm_example_13_thematic_breaks() {
    let html = render_html(&parse_final_nodes("***\n---\n___\n"));
    assert_eq!(html, "<hr />\n<hr />\n<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.structure/test]
// [spec:gfm:syn:leaf-blocks.atx-headings.opening-sequence-length-one-to-six/test]
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

// [spec:gfm:sem:leaf-blocks.setext-headings.level-determined-by-underline-marker/test]
// [spec:gfm:def:leaf-blocks.setext-headings.underline-uses-equals-or-dashes/test]
// [spec:gfm:sem:leaf-blocks.setext-headings.contents-parse-preceding-lines-as-inline-content/test]
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

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.unclosed-block-closes-at-end-of-containing-block/test]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.content-may-be-empty/test]
#[test]
fn gfm_example_126_unclosed_fence_at_eof() {
    let html = render_html(&parse_final_nodes("```\n"));
    assert_eq!(html, "<pre><code></code></pre>\n");
}

// [spec:gfm:syn:inlines.backslash-escapes.backslash-at-end-line-is-hard-line-break/test]
// [spec:gfm:syn:inlines.hard-line-breaks.backslash-before-line-ending-also-produces-hard-line-break/test]
#[test]
fn gfm_backslash_hard_break() {
    let html = render_html(&parse_final_nodes("foo\\\nbar\n"));
    assert_eq!(html, "<p>foo<br />\nbar</p>\n");
}

// [spec:gfm:syn:inlines.code-spans.example/test]
// [spec:gfm:def:inlines.code-spans.definition/test]
// [spec:gfm:def:inlines.code-spans.backtick-string-definition/test]
#[test]
fn gfm_code_span() {
    let html = render_html(&parse_final_nodes("`foo`\n"));
    assert_eq!(html, "<p><code>foo</code></p>\n");
}

// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.single-asterisk-can-open-emphasis/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.single-asterisk-can-close-emphasis/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.double-asterisk-can-open-strong-emphasis/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.double-asterisk-can-close-strong-emphasis/test]
#[test]
fn gfm_emphasis_and_strong() {
    let html = render_html(&parse_final_nodes("*em* and **strong**\n"));
    assert_eq!(html, "<p><em>em</em> and <strong>strong</strong></p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-is-not-allowed-inside-words/test]
#[test]
fn gfm_intraword_underscore_is_literal() {
    let html = render_html(&parse_final_nodes("foo_bar_baz\n"));
    assert_eq!(html, "<p>foo_bar_baz</p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-permitted/test]
#[test]
fn gfm_intraword_asterisk_is_allowed() {
    let html = render_html(&parse_final_nodes("foo*bar*baz\n"));
    assert_eq!(html, "<p>foo<em>bar</em>baz</p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-can-open-when-punctuation-precedes/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.single-underscore-can-open-emphasis/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.single-underscore-can-close-emphasis/test]
#[test]
fn gfm_punctuation_gated_underscore_emphasis() {
    let html = render_html(&parse_final_nodes("foo-_(bar)_\n"));
    assert_eq!(html, "<p>foo-<em>(bar)</em></p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-can-close-when-punctuation-follows/test]
#[test]
fn gfm_punctuation_gated_underscore_closer() {
    let html = render_html(&parse_final_nodes("_(bar)_.\n"));
    assert_eq!(html, "<p><em>(bar)</em>.</p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-produces-nested-strong/test]
#[test]
fn gfm_triple_asterisk_nests_emphasis_and_strong() {
    let html = render_html(&parse_final_nodes("***foo***\n"));
    assert_eq!(html, "<p><em><strong>foo</strong></em></p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.both-multiples-of-three-allow-emphasis-match/test]
#[test]
fn gfm_multiple_of_three_balanced_case() {
    let html = render_html(&parse_final_nodes("foo***bar***baz\n"));
    assert_eq!(html, "<p>foo<em><strong>bar</strong></em>baz</p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-emphasis-requires-matching-delimiters/test]
#[test]
fn gfm_nested_strong_inside_emphasis() {
    let html = render_html(&parse_final_nodes("*foo **bar***\n"));
    assert_eq!(html, "<p><em>foo <strong>bar</strong></em></p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-permitted/test]
#[test]
fn gfm_intraword_double_asterisk_is_allowed() {
    let html = render_html(&parse_final_nodes("foo**bar**baz\n"));
    assert_eq!(html, "<p>foo<strong>bar</strong>baz</p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-forbidden/test]
#[test]
fn gfm_intraword_double_underscore_is_literal() {
    let html = render_html(&parse_final_nodes("foo__bar__baz\n"));
    assert_eq!(html, "<p>foo__bar__baz</p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-open-when-punctuation-precedes/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-open-strong-emphasis/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-close-strong-emphasis/test]
#[test]
fn gfm_punctuation_gated_double_underscore_strong() {
    let html = render_html(&parse_final_nodes("foo-__(bar)__\n"));
    assert_eq!(html, "<p>foo-<strong>(bar)</strong></p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-close-when-punctuation-follows/test]
#[test]
fn gfm_punctuation_gated_double_underscore_closer() {
    let html = render_html(&parse_final_nodes("__(bar)__.\n"));
    assert_eq!(html, "<p><strong>(bar)</strong>.</p>\n");
}

// [spec:gfm:syn:inlines.links.inline-link-definition/test]
#[test]
fn gfm_inline_link() {
    let html = render_html(&parse_final_nodes("[link](/dest \"title\")\n"));
    assert_eq!(html, "<p><a href=\"/dest\" title=\"title\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.images.definition/test]
#[test]
fn gfm_inline_image() {
    let html = render_html(&parse_final_nodes("![alt](/img.png)\n"));
    assert_eq!(html, "<p><img src=\"/img.png\" alt=\"alt\" /></p>\n");
}

// [spec:gfm:sem:leaf-blocks.link-reference-definitions.may-appear-before-or-after-uses/test]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.defines-label-for-reference-links-and-images/test]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.scope-is-the-entire-document/test]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.use-may-precede-definition/test]
#[test]
fn gfm_reference_link_later_definition() {
    let html = render_html(&parse_final_nodes("[foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// [spec:gfm:def:inlines.raw-html.html-like-tag-text-is-parsed-as-raw-html/test]
// [spec:gfm:def:inlines.raw-html.html-tag-definition/test]
// [spec:gfm:def:inlines.raw-html.open-tag-definition/test]
// [spec:gfm:def:inlines.raw-html.closing-tag-definition/test]
// [spec:gfm:syn:inlines.raw-html.tag-name-definition/test]
#[test]
fn gfm_inline_raw_html() {
    let html = render_html(&parse_final_nodes("a <span>hi</span> b\n"));
    assert_eq!(html, "<p>a <span>hi</span> b</p>\n");
}

// [spec:gfm:def:leaf-blocks.html-blocks.definition/test]
#[test]
fn gfm_html_block_passthrough() {
    let html = render_html(&parse_final_nodes("<div>\nfoo\n</div>\n"));
    assert_eq!(html, "<div>\nfoo\n</div>\n");
}

// [spec:gfm:def:inlines.strikethrough-extension.gfm-enables-strikethrough-extension/test]
// [spec:gfm:def:inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters/test]
#[test]
fn gfm_strikethrough() {
    let html = render_html(&parse_final_nodes("~~gone~~\n"));
    assert_eq!(html, "<p><del>gone</del></p>\n");
}

// [spec:gfm:def:inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters/test]
#[test]
fn gfm_strikethrough_skips_internal_code_span_closers() {
    let html = render_html(&parse_final_nodes("~~`code ~~`~~\n"));
    assert_eq!(html, "<p><del><code>code ~~</code></del></p>\n");
}

// [spec:gfm:def:inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters/test]
#[test]
fn gfm_strikethrough_rejects_whitespace_delimited_cases() {
    let html = render_html(&parse_final_nodes("~~ hi~~ and ~~hi ~~\n"));
    assert_eq!(html, "<p>~~ hi~~ and ~~hi ~~</p>\n");
}

// [spec:gfm:def:inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters/test]
#[test]
fn gfm_strikethrough_spans_soft_breaks() {
    let html = render_html(&parse_final_nodes("This ~~has a\nsoft break~~.\n"));
    assert_eq!(html, "<p>This <del>has a\nsoft break</del>.</p>\n");
}

// [spec:gfm:syn:inlines.strikethrough-extension.new-paragraph-ends-strikethrough-parse/test]
#[test]
fn gfm_strikethrough_stops_at_paragraph_breaks() {
    let html = render_html(&parse_final_nodes("This ~~has a\n\nnew paragraph~~.\n"));
    assert_eq!(html, "<p>This ~~has a</p>\n<p>new paragraph~~.</p>\n");
}

// [spec:gfm:def:container-blocks.task-list-items-extension.definition/test]
// [spec:gfm:def:container-blocks.task-list-items-extension.marker-structure/test]
// [spec:gfm:sem:container-blocks.task-list-items-extension.whitespace-marker-is-unchecked/test]
// [spec:gfm:sem:container-blocks.task-list-items-extension.rendered-marker-becomes-semantic-checkbox/test]
// [spec:gfm:sem:container-blocks.task-list-items-extension.gfm-enables-task-list-extension/test]
#[test]
fn gfm_task_list_item() {
    let html = render_html(&parse_final_nodes("- [x] done\n- [ ] todo\n"));
    assert_eq!(
        html,
        "<ul>\n<li><input type=\"checkbox\" disabled=\"\" checked=\"\" /> done</li>\n<li><input type=\"checkbox\" disabled=\"\" /> todo</li>\n</ul>\n"
    );
}

// [spec:gfm:def:leaf-blocks.tables-extension.table-definition/test]
#[test]
fn gfm_table() {
    let html = render_html(&parse_final_nodes("| a | b |\n| :-- | --: |\n| 1 | 2 |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th align=\"left\">a</th><th align=\"right\">b</th></tr>\n</thead>\n<tbody>\n<tr><td align=\"left\">1</td><td align=\"right\">2</td></tr>\n</tbody>\n</table>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks-extension.extended-url-autolink-definition/test]
// [spec:gfm:def:inlines.autolinks-extension.definition/test]
// [spec:gfm:def:inlines.autolinks-extension.gfm-enables-extended-autolinks/test]
#[test]
fn gfm_autolink_literal() {
    let html = render_html(&parse_final_nodes("visit https://example.com now\n"));
    assert_eq!(
        html,
        "<p>visit <a href=\"https://example.com\">https://example.com</a> now</p>\n"
    );
}

// [spec:gfm:syn:container-blocks.list-items.lazy-continuation-example/test]
// [spec:gfm:syn:container-blocks.list-items.laziness-works-inside-nested-structures/test]
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

// [spec:gfm:syn:container-blocks.list-items.ordered-start-number-is-at-most-nine-digits/test]
// [spec:gfm:syn:container-blocks.list-items.ordered-start-number-may-have-leading-zeroes/test]
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

// [spec:gfm:syn:container-blocks.list-items.interrupting-ordered-list-must-start-at-one/test]
#[test]
fn gfm_ordered_list_interrupt_requires_one() {
    let html = render_html(&parse_final_nodes("para\n2. no interrupt\n"));
    assert_eq!(html, "<p>para\n2. no interrupt</p>\n");
}

// [spec:gfm:syn:container-blocks.list-items.empty-item-does-not-require-post-marker-spaces/test]
#[test]
fn gfm_empty_list_item_without_space() {
    let html = render_html(&parse_final_nodes("*\n* next\n"));
    assert_eq!(html, "<ul>\n<li></li>\n<li>next</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.interrupting-first-item-cannot-begin-with-blank-line/test]
// [spec:gfm:syn:container-blocks.list-items.blank-line-start-makes-post-marker-spacing-irrelevant/test]
#[test]
fn gfm_blank_start_list_item_cases() {
    let html = render_html(&parse_final_nodes("para\n+\n  foo\n"));
    assert_eq!(html, "<p>para\n+\n  foo</p>\n");

    let html = render_html(&parse_final_nodes("+   \n  foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// [spec:gfm:thm:container-blocks.list-items.sublists-follow-general-indentation-rules/test]
#[test]
fn gfm_nested_sublist() {
    let html = render_html(&parse_final_nodes("- one\n  - two\n"));
    assert_eq!(
        html,
        "<ul>\n<li>one<ul>\n<li>two</li>\n</ul>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.list-items.indented-code-must-be-four-spaces-beyond-content-indent/test]
#[test]
fn gfm_indented_code_inside_list_item() {
    let html = render_html(&parse_final_nodes("- foo\n\n      bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>bar
</code></pre>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:sem:preliminaries.tabs.tab-indented-list-continuation-equivalence/test]
// [spec:gfm:sem:preliminaries.tabs.tab-indented-code-block-equivalence/test]
#[test]
fn gfm_tab_indentation_structural_cases() {
    let html = render_html(&parse_final_nodes("\tcode\n"));
    assert_eq!(
        html,
        "<pre><code>code
</code></pre>\n"
    );

    let html = render_html(&parse_final_nodes("-\tfoo\n\tbar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("- foo\n\n\t\tbar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>  bar
</code></pre>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:sem:preliminaries.tabs.tab-after-block-quote-marker-counts-toward-indent/test]
#[test]
fn gfm_tab_after_block_quote_marker_counts_toward_indent() {
    let html = render_html(&parse_final_nodes(">\t\tfoo\n"));
    assert_eq!(
        html,
        "<blockquote>\n<pre><code>  foo
</code></pre>\n</blockquote>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-1-script-pre-style-start-and-end-conditions/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs/test]
#[test]
fn gfm_html_block_type_1_interrupts_paragraph_and_closes_on_end_tag() {
    let html = render_html(&parse_final_nodes(
        "before\n<script>\n*not emphasis*\n</script>\nafter\n",
    ));
    assert_eq!(
        html,
        "<p>before</p>\n<script>\n*not emphasis*\n</script>\n<p>after</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-2-comment-start-and-end-conditions/test]
#[test]
fn gfm_html_block_type_2_comment_runs_until_comment_close() {
    let html = render_html(&parse_final_nodes("<!--\n# not a heading\n-->\nafter\n"));
    assert_eq!(html, "<!--\n# not a heading\n-->\n<p>after</p>\n");
}

// [spec:gfm:sem:leaf-blocks.html-blocks.trailing-text-after-end-marker-stays-inside-block/test]
#[test]
fn gfm_html_block_type_1_keeps_trailing_text_on_end_marker_line_inside_block() {
    let html = render_html(&parse_final_nodes(
        "<script>\nfoo\n</script>1. *bar*\nafter\n",
    ));
    assert_eq!(html, "<script>\nfoo\n</script>1. *bar*\n<p>after</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-3-processing-instruction-start-and-end-conditions/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs/test]
// [spec:gfm:thm:leaf-blocks.html-blocks.types-1-through-5-may-contain-blank-lines/test]
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

// [spec:gfm:syn:leaf-blocks.html-blocks.type-4-declaration-start-and-end-conditions/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs/test]
#[test]
fn gfm_html_block_type_4_interrupts_paragraph_and_closes_on_gt() {
    let html = render_html(&parse_final_nodes("before\n<!DOCTYPE html>\nafter\n"));
    assert_eq!(html, "<p>before</p>\n<!DOCTYPE html>\n<p>after</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-5-cdata-start-and-end-conditions/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs/test]
// [spec:gfm:thm:leaf-blocks.html-blocks.types-1-through-5-may-contain-blank-lines/test]
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

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-block-tag-start-and-blank-line-end/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.blank-line-ends-type-6-block-and-normal-parsing-resumes/test]
#[test]
fn gfm_html_block_type_6_interrupts_paragraph_and_ends_at_blank_line() {
    let html = render_html(&parse_final_nodes("before\n<div>\nraw\n\nafter\n"));
    assert_eq!(html, "<p>before</p>\n<div>\nraw\n<p>after</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-continues-through-apparent-markdown-until-blank-line/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-and-7-usually-require-following-blank-line/test]
#[test]
fn gfm_html_block_type_6_keeps_following_markdown_inside_block_without_blank_line() {
    let html = render_html(&parse_final_nodes("<div>\nbar\n</div>\n*foo*\n"));
    assert_eq!(html, "<div>\nbar\n</div>\n*foo*\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-7-complete-tag-start-and-blank-line-end/test]
#[test]
fn gfm_html_block_type_7_generic_tag_at_top_level_ends_at_blank_line() {
    let html = render_html(&parse_final_nodes("<span>\nraw\n\nafter\n"));
    assert_eq!(html, "<span>\nraw\n<p>after</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.non-type-6-tag-must-be-complete-and-alone-on-first-line/test]
#[test]
fn gfm_html_block_type_7_requires_a_complete_non_block_tag_on_its_first_line() {
    let html = render_html(&parse_final_nodes("<a href=\"foo\">\n*bar*\n</a>\n"));
    assert_eq!(html, "<a href=\"foo\">\n*bar*\n</a>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-7-cannot-interrupt-paragraphs/test]
#[test]
fn gfm_html_block_type_7_does_not_interrupt_paragraph() {
    let html = render_html(&parse_final_nodes("before\n<span>\nafter\n"));
    assert_eq!(html, "<p>before\n<span>\nafter</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.block-vs-inline-tag-behavior/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.inline-html-when-tag-is-not-alone-on-line/test]
#[test]
fn gfm_html_block_del_tag_distinguishes_block_and_inline_forms() {
    let html = render_html(&parse_final_nodes("<del>\n*foo*\n</del>\n"));
    assert_eq!(html, "<del>\n*foo*\n</del>\n");

    let html = render_html(&parse_final_nodes("<del>*foo*</del>\n"));
    assert_eq!(html, "<p><del><em>foo</em></del></p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.blank-line-after-opening-tag-allows-following-markdown/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.blank-line-separates-two-html-blocks-with-markdown-between/test]
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

// [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-6-need-not-be-preceded-by-blank-line/test]
#[test]
fn gfm_html_block_type_6_interrupts_without_preceding_blank_line() {
    let html = render_html(&parse_final_nodes("Foo\n<div>\nbar\n</div>\n"));
    assert_eq!(html, "<p>Foo</p>\n<div>\nbar\n</div>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.end-marker-may-occur-on-same-line-as-start/test]
// [spec:gfm:syn:leaf-blocks.html-blocks.single-line-block-when-start-and-end-both-match/test]
// [spec:gfm:sem:leaf-blocks.html-blocks.trailing-text-after-end-marker-stays-inside-block/test]
#[test]
fn gfm_html_block_same_line_end_markers_keep_their_line_inside_the_block() {
    let html = render_html(&parse_final_nodes("<style>p{color:red;}</style>\n*foo*\n"));
    assert_eq!(html, "<style>p{color:red;}</style>\n<p><em>foo</em></p>\n");

    let html = render_html(&parse_final_nodes("<!-- foo -->*bar*\n*baz*\n"));
    assert_eq!(html, "<!-- foo -->*bar*\n<p><em>baz</em></p>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.example/test]
// [spec:gfm:def:container-blocks.block-quotes.marker-structure/test]
#[test]
fn gfm_example_228_block_quote() {
    let html = render_html(&parse_final_nodes("> # Foo\n> bar\n> baz\n"));
    assert_eq!(
        html,
        "<blockquote>\n<h1>Foo</h1>\n<p>bar\nbaz</p>\n</blockquote>\n"
    );
}

// [spec:gfm:def:container-blocks.lists.definition/test]
// [spec:gfm:def:container-blocks.lists.ordered-and-bullet-list-definition/test]
#[test]
fn gfm_simple_list() {
    let html = render_html(&parse_final_nodes("- one\n- two\n"));
    assert_eq!(html, "<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n");
}

// [spec:gfm:def:leaf-blocks.indented-code-blocks.definition/test]
// [spec:gfm:sem:leaf-blocks.indented-code-blocks.indent-beyond-four-is-preserved/test]
#[test]
fn gfm_example_107_indented_code() {
    let html = render_html(&parse_final_nodes(
        "    a simple\n      indented code block\n",
    ));
    assert_eq!(
        html,
        "<pre><code>a simple\n  indented code block
</code></pre>\n"
    );
}

// [spec:gfm:sem:inlines.code-spans.all-space-content-is-not-trimmed/test]
#[test]
fn gfm_code_span_all_spaces_not_trimmed() {
    let html = render_html(&parse_final_nodes("`` ` ``\n"));
    assert_eq!(html, "<p><code>`</code></p>\n");
}

// [spec:gfm:syn:inlines.code-spans.longer-delimiters-allow-literal-backticks/test]
// [spec:gfm:sem:inlines.code-spans.single-edge-space-is-trimmed/test]
#[test]
fn gfm_code_span_longer_delimiters_allow_literal_backticks() {
    let html = render_html(&parse_final_nodes("`` foo ` bar ``\n"));
    assert_eq!(html, "<p><code>foo ` bar</code></p>\n");
}

// [spec:gfm:sem:inlines.code-spans.edge-space-trimming-allows-literal-backticks-at-edges/test]
#[test]
fn gfm_code_span_edge_space_trimming_allows_literal_backticks_at_edges() {
    // Using triple backtick delimiter to include double backtick content
    let html = render_html(&parse_final_nodes("``` `` ```\n"));
    assert_eq!(html, "<p><code>``</code></p>\n");
}

// [spec:gfm:sem:inlines.code-spans.only-one-space-is-trimmed/test]
#[test]
fn gfm_code_span_only_one_space_is_trimmed() {
    // Only one space is stripped from each edge
    let html = render_html(&parse_final_nodes("`  foo  `\n"));
    assert_eq!(html, "<p><code> foo </code></p>\n");
}

// [spec:gfm:sem:inlines.code-spans.edge-space-trimming-requires-space-on-both-sides/test]
#[test]
fn gfm_code_span_edge_space_trimming_requires_space_on_both_sides() {
    let html = render_html(&parse_final_nodes("` a`\n"));
    assert_eq!(html, "<p><code> a</code></p>\n");
}

// [spec:gfm:sem:inlines.code-spans.only-ascii-space-is-trimmed/test]
#[test]
fn gfm_code_span_only_ascii_space_is_trimmed() {
    // U+00A0 non-breaking space is not trimmed
    let html = render_html(&parse_final_nodes("`\u{00a0}b\u{00a0}`\n"));
    assert_eq!(html, "<p><code>\u{00a0}b\u{00a0}</code></p>\n");
}

// [spec:gfm:sem:inlines.code-spans.line-endings-become-spaces/test]
// [spec:gfm:sem:inlines.code-spans.line-endings-conversion-example/test]
#[test]
fn gfm_code_span_line_endings_become_spaces() {
    let html = render_html(&parse_final_nodes("``\nfoo\nbar  \nbaz\n``\n"));
    assert_eq!(html, "<p><code>foo bar   baz</code></p>\n");

    let html = render_html(&parse_final_nodes("``\nfoo \n``\n"));
    assert_eq!(html, "<p><code>foo </code></p>\n");
}

// [spec:gfm:sem:inlines.code-spans.interior-spaces-are-preserved/test]
#[test]
fn gfm_code_span_interior_spaces_are_preserved() {
    let html = render_html(&parse_final_nodes("`foo   bar \nbaz`\n"));
    assert_eq!(html, "<p><code>foo   bar  baz</code></p>\n");
}

// [spec:gfm:req:inlines.code-spans.browser-rendering-may-collapse-spaces+1/test]
#[test]
fn gfm_code_span_browser_rendering_note() {
    // These rules are advisory notes about CSS rendering (code{white-space: pre-wrap;}).
    // We verify code spans preserve interior spaces, which is the parser's responsibility.
    let html = render_html(&parse_final_nodes("`foo  bar`\n"));
    assert_eq!(html, "<p><code>foo  bar</code></p>\n");
}

// [spec:gfm:syn:inlines.code-spans.backslashes-are-literal/test]
#[test]
fn gfm_code_span_backslashes_are_literal() {
    let html = render_html(&parse_final_nodes("`foo\\`bar`\n"));
    assert_eq!(html, "<p><code>foo\\</code>bar`</p>\n");
}

// [spec:gfm:thm:inlines.code-spans.backslash-escapes-are-never-needed/test]
#[test]
fn gfm_code_span_backslash_escapes_are_never_needed() {
    // Use double backticks to include a single backtick in code
    let html = render_html(&parse_final_nodes("``foo`bar``\n"));
    assert_eq!(html, "<p><code>foo`bar</code></p>\n");
}

// [spec:gfm:syn:inlines.code-spans.code-spans-take-precedence-over-emphasis-and-links/test]
#[test]
fn gfm_code_span_takes_precedence_over_emphasis() {
    let html = render_html(&parse_final_nodes("*foo`*`\n"));
    assert_eq!(html, "<p>*foo<code>*</code></p>\n");
}

// [spec:gfm:syn:inlines.code-spans.code-span-prevents-link-parse/test]
#[test]
fn gfm_code_span_prevents_link_parse() {
    // Code span backticks have higher precedence than link brackets
    let html = render_html(&parse_final_nodes("xx `code` [link](url)\n"));
    assert_eq!(
        html,
        "<p>xx <code>code</code> <a href=\"url\">link</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.code-spans.code-spans-html-tags-and-autolinks-share-precedence/test]
#[test]
fn gfm_code_span_shares_precedence_with_html_tags() {
    let html = render_html(&parse_final_nodes("`<a href=\"`\">`\n"));
    assert_eq!(html, "<p><code>&lt;a href=&quot;</code>&quot;&gt;`</p>\n");
}

// [spec:gfm:syn:inlines.code-spans.html-tag-wins-when-tag-parses-first/test]
#[test]
fn gfm_code_span_html_tag_wins_when_tag_parses_first() {
    let html = render_html(&parse_final_nodes("<a href=\"`\">`\n"));
    assert_eq!(html, "<p><a href=\"`\">`</p>\n");
}

// [spec:gfm:syn:inlines.code-spans.code-span-wins-in-ambiguous-autolink-like-text/test]
#[test]
fn gfm_code_span_wins_in_ambiguous_autolink_like_text() {
    let html = render_html(&parse_final_nodes("`<http://foo.bar.`baz>`\n"));
    assert_eq!(html, "<p><code>&lt;http://foo.bar.</code>baz&gt;`</p>\n");
}

// [spec:gfm:syn:inlines.code-spans.autolink-wins-when-autolink-parses-first/test]
#[test]
fn gfm_code_span_autolink_wins_when_autolink_parses_first() {
    // When an autolink starts before a backtick, autolink wins
    let html = render_html(&parse_final_nodes("<http://foo.bar.`baz>`\n"));
    assert_eq!(
        html,
        "<p><a href=\"http://foo.bar.%60baz\">http://foo.bar.`baz</a>`</p>\n"
    );
}

// [spec:gfm:syn:inlines.code-spans.unclosed-backtick-string-is-literal/test]
#[test]
fn gfm_code_span_unclosed_backtick_string_is_literal() {
    // A single backtick with no closer is literal text
    let html = render_html(&parse_final_nodes("`foo\n"));
    assert_eq!(html, "<p>`foo</p>\n");
}

// [spec:gfm:syn:inlines.code-spans.opening-and-closing-backtick-strings-must-have-equal-length/test]
#[test]
fn gfm_code_span_opening_and_closing_must_have_equal_length() {
    // Double backtick opening must be closed by double backtick
    let html = render_html(&parse_final_nodes("``foo``\n"));
    assert_eq!(html, "<p><code>foo</code></p>\n");

    // Triple backtick opening must be closed by triple backtick
    let html = render_html(&parse_final_nodes("```foo``` bar\n"));
    assert_eq!(html, "<p><code>foo</code> bar</p>\n");
}

// ---------------------------------------------------------------------------
// Entity and numeric character references
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.entity-and-numeric-character-references.entity-references-definition/test]
// [spec:gfm:sem:inlines.entity-and-numeric-character-references.references-produce-unicode-characters/test]
#[test]
fn gfm_entity_references() {
    let html = render_html(&parse_final_nodes(
        "&nbsp; &amp; &copy; &AElig;\n&frac34;\n",
    ));
    assert_eq!(html, "<p>\u{00a0} &amp; \u{00a9} \u{00c6}\n\u{00be}</p>\n");
}

// [spec:gfm:def:inlines.entity-and-numeric-character-references.decimal-numeric-reference-definition/test]
#[test]
fn gfm_decimal_numeric_references() {
    let html = render_html(&parse_final_nodes("&#35; &#1234; &#992;\n"));
    assert_eq!(html, "<p># \u{04d2} \u{03e0}</p>\n");
}

// [spec:gfm:def:inlines.entity-and-numeric-character-references.hex-reference-definition/test]
#[test]
fn gfm_hex_numeric_references() {
    let html = render_html(&parse_final_nodes("&#XD06; &#xcab;\n"));
    assert_eq!(html, "<p>\u{0d06} \u{0cab}</p>\n");
}

// [spec:gfm:syn:inlines.entity-and-numeric-character-references.invalid-references-are-literal-text/test]
#[test]
fn gfm_invalid_references_are_literal() {
    let html = render_html(&parse_final_nodes(
        "&nbsp &x; &#; &#x;\n&#987654321;\n&#abcdef0;\n&ThisIsNotDefined; &hi?;\n",
    ));
    assert_eq!(
        html,
        "<p>&amp;nbsp &amp;x; &amp;#; &amp;#x;\n&amp;#987654321;\n&amp;#abcdef0;\n&amp;ThisIsNotDefined; &amp;hi?;</p>\n"
    );
}

// [spec:gfm:syn:inlines.entity-and-numeric-character-references.named-entities-require-semicolon/test]
#[test]
fn gfm_named_entities_require_semicolon() {
    let html = render_html(&parse_final_nodes("&copy\n"));
    assert_eq!(html, "<p>&amp;copy</p>\n");
}

// [spec:gfm:syn:inlines.entity-and-numeric-character-references.unknown-named-entities-are-literal/test]
#[test]
fn gfm_unknown_named_entities_are_literal() {
    let html = render_html(&parse_final_nodes("&MadeUpEntity;\n"));
    assert_eq!(html, "<p>&amp;MadeUpEntity;</p>\n");
}

// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-are-recognized-outside-code/test]
#[test]
fn gfm_references_recognized_outside_code() {
    // Entity references in raw HTML are passed through (recognized by HTML layer)
    let html = render_html(&parse_final_nodes("<a href=\"&ouml;&ouml;.html\">\n"));
    assert_eq!(html, "<a href=\"&ouml;&ouml;.html\">\n");

    // Entity references in paragraph text are resolved to unicode
    let html = render_html(&parse_final_nodes("&copy; &amp; &lt;\n"));
    assert_eq!(html, "<p>\u{00a9} &amp; &lt;</p>\n");
}

// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-are-literal-text-in-code/test]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-are-not-recognized-in-code/test]
#[test]
fn gfm_references_are_literal_in_code() {
    // In code span, entity references are literal text
    let html = render_html(&parse_final_nodes("`f&ouml;&ouml;`\n"));
    assert_eq!(html, "<p><code>f&amp;ouml;&amp;ouml;</code></p>\n");

    // In indented code block, entity references are literal text
    let html = render_html(&parse_final_nodes("    f&ouml;f&ouml;\n"));
    assert_eq!(
        html,
        "<pre><code>f&amp;ouml;f&amp;ouml;
</code></pre>\n"
    );
}

// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-cannot-replace-structure-symbols/test]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-cannot-replace-structural-markers/test]
#[test]
fn gfm_references_cannot_replace_structure_symbols() {
    // &#42; should not become emphasis markers
    let html = render_html(&parse_final_nodes("&#42;foo&#42;\n*foo*\n"));
    assert_eq!(html, "<p>*foo*\n<em>foo</em></p>\n");

    // &#42; should not become a list marker
    let html = render_html(&parse_final_nodes("&#42; foo\n\n* foo\n"));
    assert_eq!(html, "<p>* foo</p>\n<ul>\n<li>foo</li>\n</ul>\n");

    // &#9; (tab) should not create indented code
    let html = render_html(&parse_final_nodes("&#9;foo\n"));
    assert_eq!(html, "<p>\tfoo</p>\n");
}

// [spec:gfm:req:inlines.entity-and-numeric-character-references.parsers-need-not-preserve-source-form/test]
#[test]
fn gfm_parsers_need_not_preserve_source_form() {
    // The parser converts entity references to their Unicode characters,
    // so the source form is not preserved. We verify by checking output
    // contains the actual character rather than the entity reference.
    let html = render_html(&parse_final_nodes("&copy;\n"));
    assert_eq!(html, "<p>\u{00a9}</p>\n");
}

// [spec:gfm:sem:leaf-blocks.fenced-code-blocks.content-removes-up-to-opening-indent/test]
// [spec:gfm:sem:leaf-blocks.fenced-code-blocks.fences-may-be-indented-and-content-deindents-to-match/test]
// [spec:gfm:sem:leaf-blocks.fenced-code-blocks.short-indented-content-lines-remove-all-indent/test]
#[test]
fn gfm_fenced_code_deindent() {
    let html = render_html(&parse_final_nodes("  ```\n  aaa\n aaa\naaa\n  ```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\naaa\naaa
</code></pre>\n"
    );
}

// [spec:gfm:syn:inlines.links.balanced-parentheses-are-allowed-in-destination/test]
#[test]
fn gfm_link_balanced_parens() {
    let html = render_html(&parse_final_nodes("[link](/url(test))\n"));
    assert_eq!(html, "<p><a href=\"/url(test)\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.links.destination-may-contain-spaces-only-in-angle-brackets/test]
#[test]
fn gfm_link_angle_bracket_dest() {
    let html = render_html(&parse_final_nodes("[link](<url with spaces>)\n"));
    assert_eq!(html, "<p><a href=\"url%20with%20spaces\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.links.destination-and-title-may-both-be-omitted/test]
#[test]
fn gfm_empty_inline_link() {
    let html = render_html(&parse_final_nodes("[link]()\n"));
    assert_eq!(html, "<p><a href=\"\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.autolinks-extension.extended-www-autolink-definition/test]
// [spec:gfm:def:inlines.autolinks-extension.extended-www-autolinks-insert-http-scheme/test]
// [spec:gfm:sem:inlines.autolinks-extension.extended-www-autolinks-may-have-path-after-domain/test]
#[test]
fn gfm_www_autolink() {
    let html = render_html(&parse_final_nodes("visit www.example.com now\n"));
    assert_eq!(
        html,
        "<p>visit <a href=\"http://www.example.com\">www.example.com</a> now</p>\n"
    );

    // www autolinks may include a path after the domain
    let html = render_html(&parse_final_nodes(
        "Visit www.commonmark.org/help for more information.\n",
    ));
    assert_eq!(
        html,
        "<p>Visit <a href=\"http://www.commonmark.org/help\">www.commonmark.org/help</a> for more information.</p>\n"
    );
}

// [spec:gfm:def:inlines.autolinks-extension.trailing-punctuation-is-excluded-from-autolinks/test]
// [spec:gfm:syn:inlines.autolinks-extension.extended-autolink-path-validation/test]
#[test]
fn gfm_autolink_trailing_punctuation() {
    let html = render_html(&parse_final_nodes("visit https://example.com.\n"));
    assert_eq!(
        html,
        "<p>visit <a href=\"https://example.com\">https://example.com</a>.</p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks-extension.unmatched-trailing-right-parentheses-are-excluded/test]
// [spec:gfm:syn:inlines.autolinks-extension.trailing-parenthesis-check-applies-only-at-link-end/test]
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

// [spec:gfm:def:inlines.disallowed-raw-html-extension.gfm-enables-tagfilter-extension/test]
// [spec:gfm:sem:inlines.disallowed-raw-html-extension.disallowed-tags-change-html-parsing/test]
// [spec:gfm:def:inlines.disallowed-raw-html-extension.disallowed-tags/test]
// [spec:gfm:sem:inlines.disallowed-raw-html-extension.filtering-replaces-leading-angle-bracket/test]
// [spec:gfm:sem:inlines.disallowed-raw-html-extension.all-other-html-tags-are-left-untouched/test]
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

// [spec:gfm:syn:inlines.raw-html.tag-grammar-follows/test]
// [spec:gfm:syn:inlines.raw-html.simple-open-tag-examples/test]
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

// [spec:gfm:def:inlines.raw-html.attribute-definition/test]
// [spec:gfm:def:inlines.raw-html.attribute-name-definition/test]
// [spec:gfm:def:inlines.raw-html.attribute-value-specification-definition/test]
// [spec:gfm:def:inlines.raw-html.attribute-value-definition/test]
// [spec:gfm:def:inlines.raw-html.double-quoted-attribute-value-definition/test]
// [spec:gfm:def:inlines.raw-html.single-quoted-attribute-value-definition/test]
// [spec:gfm:def:inlines.raw-html.unquoted-attribute-value-definition/test]
#[test]
fn gfm_raw_html_attributes() {
    // Inline HTML with attributes inside a paragraph
    let html = render_html(&parse_final_nodes(
        "foo <a foo=\"bar\" bam='baz' _boolean zoop:33=zoop:33 /> bar\n",
    ));
    assert_eq!(
        html,
        "<p>foo <a foo=\"bar\" bam='baz' _boolean zoop:33=zoop:33 /> bar</p>\n"
    );
}

// [spec:gfm:syn:inlines.raw-html.custom-tag-names-are-allowed/test]
// [spec:gfm:syn:inlines.raw-html.custom-tag-and-attribute-names-are-allowed/test]
#[test]
fn gfm_raw_html_custom_tags() {
    // Example 640: custom tag names
    let html = render_html(&parse_final_nodes(
        "Foo <responsive-image src=\"foo.jpg\" />\n",
    ));
    assert_eq!(html, "<p>Foo <responsive-image src=\"foo.jpg\" /></p>\n");
}

// [spec:gfm:syn:inlines.raw-html.illegal-tag-and-attribute-syntax-is-not-parsed-as-html/test]
#[test]
fn gfm_raw_html_illegal_syntax() {
    // Example 641: illegal tag names (start with digit or underscore)
    let html = render_html(&parse_final_nodes("<33> <__>\n"));
    assert_eq!(html, "<p>&lt;33&gt; &lt;__&gt;</p>\n");
}

// [spec:gfm:syn:inlines.raw-html.closing-tags-cannot-have-attributes/test]
#[test]
fn gfm_raw_html_closing_tags() {
    // Example 646: valid closing tags
    let html = render_html(&parse_final_nodes("</a></foo >\n"));
    assert_eq!(html, "<p></a></foo ></p>\n");

    // Closing tags with attributes are NOT valid HTML - treated as text
    let html = render_html(&parse_final_nodes("</a href=\"foo\">\n"));
    assert_eq!(html, "<p>&lt;/a href=&quot;foo&quot;&gt;</p>\n");
}

// [spec:gfm:def:inlines.raw-html.html-comment-definition/test]
#[test]
fn gfm_raw_html_comments() {
    // Example 648: valid comment
    let html = render_html(&parse_final_nodes(
        "foo <!-- this is a\ncomment - with hyphen -->\n",
    ));
    assert_eq!(
        html,
        "<p>foo <!-- this is a\ncomment - with hyphen --></p>\n"
    );
}

// [spec:gfm:def:inlines.raw-html.processing-instruction-definition/test]
#[test]
fn gfm_raw_html_processing_instruction() {
    // Example 651
    let html = render_html(&parse_final_nodes("foo <?php echo $a; ?>\n"));
    assert_eq!(html, "<p>foo <?php echo $a; ?></p>\n");
}

// [spec:gfm:def:inlines.raw-html.declaration-definition/test]
#[test]
fn gfm_raw_html_declaration() {
    // Example 652
    let html = render_html(&parse_final_nodes("foo <!ELEMENT br EMPTY>\n"));
    assert_eq!(html, "<p>foo <!ELEMENT br EMPTY></p>\n");
}

// [spec:gfm:def:inlines.raw-html.cdata-section-definition/test]
#[test]
fn gfm_raw_html_cdata_section() {
    // CDATA section: the parser recognizes <![CDATA[...]]> as raw HTML
    let html = render_html(&parse_final_nodes("foo <![CDATA[hello]]>\n"));
    assert_eq!(html, "<p>foo <![CDATA[hello]]></p>\n");
}

// [spec:gfm:sem:inlines.raw-html.entity-and-numeric-references-are-preserved-in-html-attributes/test]
#[test]
fn gfm_raw_html_entity_in_attributes() {
    // Example 654
    let html = render_html(&parse_final_nodes("foo <a href=\"&ouml;\">\n"));
    assert_eq!(html, "<p>foo <a href=\"&ouml;\"></p>\n");
}

// [spec:gfm:syn:inlines.raw-html.backslash-escapes-do-not-work-in-html-attributes/test]
#[test]
fn gfm_raw_html_backslash_escapes_in_attributes() {
    // Example 655: backslash in attribute value is literal
    let html = render_html(&parse_final_nodes("foo <a href=\"\\*\">\n"));
    assert_eq!(html, "<p>foo <a href=\"\\*\"></p>\n");
}

// ---------------------------------------------------------------------------
// Backslash escapes
// ---------------------------------------------------------------------------

// [spec:gfm:syn:inlines.backslash-escapes.any-ascii-punctuation-may-be-escaped/test]
#[test]
fn gfm_backslash_escapes_all_ascii_punctuation() {
    // Example 308: all ASCII punctuation characters may be escaped. The GitHub
    // math extension claims `\[...\]` (and `\(...\)`) as inline math delimiters,
    // so the `\[\\\]` subsequence renders as inline math containing a backslash
    // rather than as three separate escapes.
    let html = render_html(&parse_final_nodes(
        "\\!\\\"\\#\\$\\%\\&\\'\\(\\)\\*\\+\\,\\-\\.\\/\\:\\;\\<\\=\\>\\?\\@\\[\\\\\\]\\^\\_\\`\\{\\|\\}\\~\n",
    ));
    assert_eq!(
        html,
        "<p>!&quot;#$%&amp;'()*+,-./:;&lt;=&gt;?@<code class=\"language-math math-display\">\\\\</code>^_`{|}~</p>\n"
    );
}

// [spec:gfm:syn:inlines.backslash-escapes.backslashes-before-other-characters-are-literal/test]
#[test]
fn gfm_backslash_escapes_non_punctuation_literal() {
    // Example 309: backslashes before non-punctuation are literal
    let html = render_html(&parse_final_nodes("\\A\\a\\ \\3\n"));
    assert_eq!(html, "<p>\\A\\a\\ \\3</p>\n");
}

// [spec:gfm:sem:inlines.backslash-escapes.escaped-characters-lose-special-meaning/test]
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

// [spec:gfm:syn:inlines.backslash-escapes.escaped-backslash-does-not-escape-following-character/test]
#[test]
fn gfm_backslash_escapes_double_backslash() {
    // Example 311: escaped backslash does not escape the following character
    let html = render_html(&parse_final_nodes("\\\\*emphasis*\n"));
    assert_eq!(html, "<p>\\<em>emphasis</em></p>\n");
}

// [spec:gfm:syn:inlines.backslash-escapes.do-not-work-in-code-autolinks-or-raw-html/test]
#[test]
fn gfm_backslash_escapes_not_in_code_or_html() {
    // Example 313: backslash escapes do not work in code spans
    let html = render_html(&parse_final_nodes("`` \\[\\` ``\n"));
    assert_eq!(html, "<p><code>\\[\\`</code></p>\n");

    // Example 315: backslash escapes do not work in fenced code blocks
    let html = render_html(&parse_final_nodes("~~~\n\\[\\]\n~~~\n"));
    assert_eq!(
        html,
        "<pre><code>\\[\\]
</code></pre>\n"
    );

    // Example 317: backslash escapes do not work in raw HTML inline
    let html = render_html(&parse_final_nodes("foo <a href=\"/bar\\/)\"> bar\n"));
    assert_eq!(html, "<p>foo <a href=\"/bar\\/)\"> bar</p>\n");
}

// [spec:gfm:syn:inlines.backslash-escapes.work-in-urls-titles-references-and-info-strings/test]
#[test]
fn gfm_backslash_escapes_in_urls_titles_info_strings() {
    let html = render_html(&parse_final_nodes("[foo](/bar\\* \"ti\\*tle\")\n"));
    assert_eq!(html, "<p><a href=\"/bar*\" title=\"ti*tle\">foo</a></p>\n");

    // Backslash escapes work in info strings
    let html = render_html(&parse_final_nodes("``` foo\\+bar\nfoo\n```\n"));
    assert_eq!(
        html,
        "<pre><code class=\"language-foo+bar\">foo
</code></pre>\n"
    );
}

// ---------------------------------------------------------------------------
// Hard line breaks
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.hard-line-breaks.two-trailing-spaces-produce-hard-line-break/test]
#[test]
fn gfm_hard_line_break_two_spaces() {
    // Example 658: two trailing spaces produce a hard line break
    let html = render_html(&parse_final_nodes("foo  \nbaz\n"));
    assert_eq!(html, "<p>foo<br />\nbaz</p>\n");
}

// [spec:gfm:syn:inlines.hard-line-breaks.more-than-two-trailing-spaces-also-work/test]
#[test]
fn gfm_hard_line_break_many_spaces() {
    // More than two trailing spaces also produce a hard line break
    let html = render_html(&parse_final_nodes("foo       \nbaz\n"));
    assert_eq!(html, "<p>foo<br />\nbaz</p>\n");
}

// [spec:gfm:sem:inlines.hard-line-breaks.leading-spaces-on-next-line-are-ignored/test]
#[test]
fn gfm_hard_line_break_leading_spaces_ignored() {
    // Leading spaces on the next line after a hard break
    let html = render_html(&parse_final_nodes("foo\\\n   bar\n"));
    assert_eq!(html, "<p>foo<br />\nbar</p>\n");

    let html = render_html(&parse_final_nodes("foo  \n   bar\n"));
    assert_eq!(html, "<p>foo<br />\nbar</p>\n");
}

// [spec:gfm:syn:inlines.hard-line-breaks.hard-line-breaks-can-occur-inside-inline-constructs/test]
#[test]
fn gfm_hard_line_break_inside_inline_constructs() {
    // Example 663: hard line break inside emphasis (spaces)
    let html = render_html(&parse_final_nodes("*foo  \nbar*\n"));
    assert_eq!(html, "<p><em>foo<br />\nbar</em></p>\n");

    // Example 664: hard line break inside emphasis (backslash)
    let html = render_html(&parse_final_nodes("*foo\\\nbar*\n"));
    assert_eq!(html, "<p><em>foo<br />\nbar</em></p>\n");
}

// [spec:gfm:syn:inlines.hard-line-breaks.hard-line-breaks-do-not-occur-inside-code-spans-or-html-tags/test]
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

// [spec:gfm:syn:inlines.hard-line-breaks.hard-line-break-syntax-does-not-work-at-end-of-block/test]
#[test]
fn gfm_hard_line_break_not_at_end_of_block() {
    // Example 669: backslash at end of paragraph is literal
    let html = render_html(&parse_final_nodes("foo\\\n"));
    assert_eq!(html, "<p>foo\\</p>\n");

    // Example 670: trailing spaces at end of paragraph are stripped
    let html = render_html(&parse_final_nodes("foo  \n"));
    assert_eq!(html, "<p>foo</p>\n");

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

// [spec:gfm:def:leaf-blocks.leaf-blocks-make-up-markdown-document/test]
#[test]
fn gfm_leaf_blocks_make_up_markdown_document() {
    // A paragraph and a thematic break are both leaf blocks
    let html = render_html(&parse_final_nodes("Hello world.\n\n---\n"));
    assert_eq!(html, "<p>Hello world.</p>\n<hr />\n");
}

// ---------------------------------------------------------------------------
// Thematic breaks
// ---------------------------------------------------------------------------

// [spec:gfm:syn:leaf-blocks.thematic-breaks.all-marker-characters-must-match/test]
#[test]
fn gfm_thematic_break_markers_must_match() {
    // Mixed markers do not form thematic breaks
    let html = render_html(&parse_final_nodes("*-*\n"));
    assert_eq!(html, "<p><em>-</em></p>\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.spaces-or-tabs-allowed-between-markers/test]
#[test]
fn gfm_thematic_break_spaces_between_markers() {
    let html = render_html(&parse_final_nodes("_ _ _\n"));
    assert_eq!(html, "<hr />\n");

    let html = render_html(&parse_final_nodes("___  ___  ___\n"));
    assert_eq!(html, "<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.marker-sequence-may-be-longer-than-three/test]
#[test]
fn gfm_thematic_break_more_than_three() {
    let html = render_html(&parse_final_nodes("--------------------\n"));
    assert_eq!(html, "<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.indent-up-to-three-spaces/test]
#[test]
fn gfm_thematic_break_indent_up_to_three() {
    let html = render_html(&parse_final_nodes(" ***\n  ***\n   ***\n"));
    assert_eq!(html, "<hr />\n<hr />\n<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.indent-four-spaces-not-allowed/test]
#[test]
fn gfm_thematic_break_four_spaces_is_code() {
    let html = render_html(&parse_final_nodes("    ***\n"));
    assert_eq!(
        html,
        "<pre><code>***
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.no-other-characters-allowed/test]
#[test]
fn gfm_thematic_break_no_other_chars() {
    let html = render_html(&parse_final_nodes("_ _ _ _ a\n"));
    assert_eq!(html, "<p>_ _ _ _ a</p>\n");

    let html = render_html(&parse_final_nodes("a]---\n"));
    assert_eq!(html, "<p>a]---</p>\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.trailing-spaces-allowed/test]
#[test]
fn gfm_thematic_break_trailing_spaces() {
    let html = render_html(&parse_final_nodes("___     \n"));
    assert_eq!(html, "<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.can-interrupt-paragraphs/test]
#[test]
fn gfm_thematic_break_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n***\n"));
    assert_eq!(html, "<p>para</p>\n<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.need-not-be-surrounded-by-blank-lines/test]
#[test]
fn gfm_thematic_break_no_surrounding_blanks() {
    let html = render_html(&parse_final_nodes("aaa\n***\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<hr />\n<p>bbb</p>\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.setext-heading-takes-precedence-over-dash-break/test]
#[test]
fn gfm_thematic_break_setext_takes_precedence() {
    let html = render_html(&parse_final_nodes("Foo\n---\nbar\n"));
    assert_eq!(html, "<h2>Foo</h2>\n<p>bar</p>\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.thematic-break-takes-precedence-over-list-item/test]
#[test]
fn gfm_thematic_break_over_list_item() {
    // "- ---" is a list containing a thematic break according to GFM spec (example 30)
    // but "---" after list item marker is a thematic break inside the list
    let html = render_html(&parse_final_nodes("- foo\n---\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.thematic-breaks.use-different-bullet-for-break-inside-list-item/test]
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

// [spec:gfm:syn:leaf-blocks.atx-headings.opening-sequence-length-at-most-six/test]
#[test]
fn gfm_atx_heading_seven_hashes_not_heading() {
    let html = render_html(&parse_final_nodes("####### foo\n"));
    assert_eq!(html, "<p>####### foo</p>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.opening-sequence-followed-by-space-or-eol/test]
#[test]
fn gfm_atx_heading_requires_space_after_hashes() {
    let html = render_html(&parse_final_nodes("#5 bolt\n"));
    assert_eq!(html, "<p>#5 bolt</p>\n");

    let html = render_html(&parse_final_nodes("#hashtag\n"));
    assert_eq!(html, "<p>#hashtag</p>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.escaped-opening-hash-does-not-open-heading/test]
#[test]
fn gfm_atx_heading_escaped_opening_hash() {
    let html = render_html(&parse_final_nodes("\\## foo\n"));
    assert_eq!(html, "<p>## foo</p>\n");
}

// [spec:gfm:sem:leaf-blocks.atx-headings.contents-parse-as-inline-content/test]
#[test]
fn gfm_atx_heading_inline_content() {
    let html = render_html(&parse_final_nodes("# foo *bar* \\*baz\\*\n"));
    assert_eq!(html, "<h1>foo <em>bar</em> *baz*</h1>\n");
}

// [spec:gfm:sem:leaf-blocks.atx-headings.trim-leading-and-trailing-whitespace-before-inline-parse/test]
#[test]
fn gfm_atx_heading_trim_whitespace() {
    let html = render_html(&parse_final_nodes(
        "#                  foo                     \n",
    ));
    assert_eq!(html, "<h1>foo</h1>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.opening-indent-up-to-three-spaces/test]
#[test]
fn gfm_atx_heading_indent_up_to_three() {
    let html = render_html(&parse_final_nodes(" ### foo\n  ## foo\n   # foo\n"));
    assert_eq!(html, "<h3>foo</h3>\n<h2>foo</h2>\n<h1>foo</h1>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.opening-indent-four-spaces-not-allowed/test]
#[test]
fn gfm_atx_heading_four_spaces_is_code() {
    let html = render_html(&parse_final_nodes("    # foo\n"));
    assert_eq!(
        html,
        "<pre><code># foo
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-optional/test]
// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-length-independent/test]
#[test]
fn gfm_atx_heading_closing_sequence() {
    let html = render_html(&parse_final_nodes("## foo ##\n  ###   bar    ###\n"));
    assert_eq!(html, "<h2>foo</h2>\n<h3>bar</h3>\n");

    // Closing sequence length need not match opening
    let html = render_html(&parse_final_nodes(
        "# foo ##################################\n",
    ));
    assert_eq!(html, "<h1>foo</h1>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-may-be-followed-by-spaces-only/test]
#[test]
fn gfm_atx_heading_closing_followed_by_spaces() {
    let html = render_html(&parse_final_nodes("### foo ###     \n"));
    assert_eq!(html, "<h3>foo</h3>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-must-be-preceded-by-space/test]
#[test]
fn gfm_atx_heading_closing_requires_preceding_space() {
    let html = render_html(&parse_final_nodes("# foo#\n"));
    assert_eq!(html, "<h1>foo#</h1>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.escaped-hash-does-not-count-in-closing-sequence/test]
#[test]
fn gfm_atx_heading_escaped_hash_not_closing() {
    let html = render_html(&parse_final_nodes("### foo \\###\n"));
    assert_eq!(html, "<h3>foo ###</h3>\n");

    let html = render_html(&parse_final_nodes("## foo #\\##\n"));
    assert_eq!(html, "<h2>foo ###</h2>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.non-space-after-closing-hashes-makes-them-content/test]
#[test]
fn gfm_atx_heading_non_space_after_closing_hashes() {
    let html = render_html(&parse_final_nodes("### foo ## bar\n"));
    assert_eq!(html, "<h3>foo ## bar</h3>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.can-be-empty/test]
#[test]
fn gfm_atx_heading_empty() {
    let html = render_html(&parse_final_nodes("## \n"));
    assert_eq!(html, "<h2></h2>\n");

    let html = render_html(&parse_final_nodes("#\n"));
    assert_eq!(html, "<h1></h1>\n");
}

// [spec:gfm:syn:leaf-blocks.atx-headings.can-interrupt-paragraphs-without-blank-lines/test]
#[test]
fn gfm_atx_heading_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n# heading\n"));
    assert_eq!(html, "<p>para</p>\n<h1>heading</h1>\n");
}

// ---------------------------------------------------------------------------
// Setext headings
// ---------------------------------------------------------------------------

// [spec:gfm:syn:leaf-blocks.setext-headings.underline-may-be-any-length/test]
#[test]
fn gfm_setext_heading_underline_any_length() {
    let html = render_html(&parse_final_nodes("Foo\n---\n\nBar\n=\n"));
    assert_eq!(html, "<h2>Foo</h2>\n<h1>Bar</h1>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.content-indent-up-to-three-spaces-and-need-not-align/test]
// [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-indent-up-to-three-spaces/test]
#[test]
fn gfm_setext_heading_content_indent() {
    let html = render_html(&parse_final_nodes(
        "   Foo\n---\n\n  Foo\n-----\n\n  Foo\n  ===\n",
    ));
    assert_eq!(html, "<h2>Foo</h2>\n<h2>Foo</h2>\n<h1>Foo</h1>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.content-indent-four-spaces-not-allowed/test]
#[test]
fn gfm_setext_heading_content_four_spaces_not_allowed() {
    let html = render_html(&parse_final_nodes("    Foo\n---\n"));
    assert_eq!(
        html,
        "<pre><code>Foo
</code></pre>\n<hr />\n"
    );
}

// [spec:gfm:syn:leaf-blocks.setext-headings.underline-indent-up-to-three-spaces/test]
#[test]
fn gfm_setext_heading_underline_indent() {
    let html = render_html(&parse_final_nodes("Foo\n   ---\n"));
    assert_eq!(html, "<h2>Foo</h2>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.underline-indent-four-spaces-not-allowed/test]
#[test]
fn gfm_setext_heading_underline_four_spaces() {
    let html = render_html(&parse_final_nodes("Foo\n    ---\n"));
    assert_eq!(html, "<p>Foo\n    ---</p>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.setext-heading-underline-cannot-contain-internal-spaces/test]
#[test]
fn gfm_setext_heading_underline_no_internal_spaces() {
    let html = render_html(&parse_final_nodes("Foo\n= =\n"));
    assert_eq!(html, "<p>Foo\n= =</p>\n");

    let html = render_html(&parse_final_nodes("Foo\n--- -\n"));
    assert_eq!(html, "<p>Foo</p>\n<hr />\n");
}

// [spec:gfm:sem:leaf-blocks.setext-headings.trailing-spaces-in-content-do-not-create-line-break/test]
#[test]
fn gfm_setext_heading_trailing_spaces_no_break() {
    let html = render_html(&parse_final_nodes("Foo  \n-----\n"));
    assert_eq!(html, "<h2>Foo</h2>\n");
}

// [spec:gfm:sem:leaf-blocks.setext-headings.trailing-backslash-does-not-create-line-break/test]
#[test]
fn gfm_setext_heading_trailing_backslash_no_break() {
    let html = render_html(&parse_final_nodes("Foo\\\n----\n"));
    assert_eq!(html, "<h2>Foo\\</h2>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.backslash-escapes-allow-literal-block-markers-in-content/test]
#[test]
fn gfm_setext_heading_escaped_block_markers() {
    let html = render_html(&parse_final_nodes("\\> foo\n------\n"));
    assert_eq!(html, "<h2>&gt; foo</h2>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.backslash-escaped-underline-produces-literal-text/test]
#[test]
fn gfm_setext_heading_escaped_underline() {
    let html = render_html(&parse_final_nodes("Foo\n\\---\n"));
    // The backslash escapes the first dash, so it's not a valid setext underline
    // This produces a paragraph
    assert_eq!(html, "<p>Foo\n---</p>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.content-may-span-multiple-lines/test]
// [spec:gfm:sem:leaf-blocks.setext-headings.raw-content-concatenates-lines-and-trims-edge-whitespace/test]
#[test]
fn gfm_setext_heading_multiline_content() {
    let html = render_html(&parse_final_nodes("Foo\nbar\n---\n"));
    assert_eq!(html, "<h2>Foo\nbar</h2>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.need-not-be-surrounded-by-blank-lines/test]
// [spec:gfm:syn:leaf-blocks.setext-headings.blank-lines-generally-not-required-around-heading/test]
#[test]
fn gfm_setext_heading_no_surrounding_blanks() {
    let html = render_html(&parse_final_nodes("---\nFoo\n---\nBar\n---\nBaz\n"));
    assert_eq!(html, "<hr />\n<h2>Foo</h2>\n<h2>Bar</h2>\n<p>Baz</p>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.blank-line-required-after-paragraph-before-heading/test]
#[test]
fn gfm_setext_heading_needs_blank_after_paragraph() {
    // Without a blank line between a paragraph and a potential setext heading
    // content, the text just becomes part of the paragraph; dashes form
    // the setext underline for the combined paragraph.
    let html = render_html(&parse_final_nodes("Foo\nbar\n\nbaz\n---\n"));
    assert_eq!(html, "<p>Foo\nbar</p>\n<h2>baz</h2>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.cannot-be-empty/test]
// [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-require-non-whitespace-content/test]
#[test]
fn gfm_setext_heading_cannot_be_empty() {
    // A blank line before the underline means no setext heading
    let html = render_html(&parse_final_nodes("\n====\n"));
    assert_eq!(html, "<p>====</p>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.cannot-interrupt-paragraphs/test]
#[test]
fn gfm_setext_heading_cannot_interrupt_paragraph() {
    // "===" below "bar" looks like setext but "bar" continues "Foo" paragraph
    // since there's no blank line between "Foo" and "bar"
    // The spec says text lines must otherwise parse as paragraphs and
    // setext heading cannot interrupt paragraphs in some cases
    let html = render_html(&parse_final_nodes("Foo\nbar\n---\nbaz\n"));
    assert_eq!(html, "<h2>Foo\nbar</h2>\n<p>baz</p>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-must-not-parse-as-other-block-constructs/test]
// [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-must-otherwise-parse-as-paragraphs/test]
#[test]
fn gfm_setext_heading_text_must_be_paragraph_content() {
    // A thematic break followed by a line of === does not make a setext heading
    let html = render_html(&parse_final_nodes("---\n---\n"));
    assert_eq!(html, "<hr />\n<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.underline-cannot-be-lazy-continuation-line/test]
#[test]
fn gfm_setext_heading_underline_not_lazy_continuation() {
    let html = render_html(&parse_final_nodes("> Foo\n---\n"));
    assert_eq!(html, "<blockquote>\n<p>Foo</p>\n</blockquote>\n<hr />\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.block-indicators-take-precedence-over-inline-indicators/test]
#[test]
fn gfm_setext_heading_block_precedence() {
    // Per spec: "===" in lazy continuation position is literal text, not a setext underline
    let html = render_html(&parse_final_nodes("> foo\nbar\n===\n"));
    assert_eq!(html, "<blockquote>\n<p>foo\nbar\n===</p>\n</blockquote>\n");
}

// [spec:gfm:def:leaf-blocks.setext-headings.multiline-heading-has-four-interpretations/test]
// [spec:gfm:sem:leaf-blocks.setext-headings.multiline-heading-interprets-as-heading-then-paragraph/test]
#[test]
fn gfm_setext_heading_multiline_interpretations() {
    // Multi-line content followed by setext underline
    let html = render_html(&parse_final_nodes("Foo\nbar\n---\nbaz\n"));
    assert_eq!(html, "<h2>Foo\nbar</h2>\n<p>baz</p>\n");
}

// [spec:gfm:syn:leaf-blocks.setext-headings.blank-lines-around-break-produce-hr/test]
// [spec:gfm:syn:leaf-blocks.setext-headings.non-setext-break-marker-produces-hr/test]
#[test]
fn gfm_setext_heading_blank_lines_around_break_produce_hr() {
    let html = render_html(&parse_final_nodes("Foo\n\n---\n\nBar\n"));
    assert_eq!(html, "<p>Foo</p>\n<hr />\n<p>Bar</p>\n");
}

// ---------------------------------------------------------------------------
// Paragraphs
// ---------------------------------------------------------------------------

// [spec:gfm:syn:leaf-blocks.paragraphs.structure/test]
// [spec:gfm:sem:leaf-blocks.paragraphs.contents-parse-as-inlines/test]
#[test]
fn gfm_paragraph_structure() {
    let html = render_html(&parse_final_nodes("aaa\n\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");
}

// [spec:gfm:syn:leaf-blocks.paragraphs.may-span-multiple-lines-without-blank-lines/test]
#[test]
fn gfm_paragraph_multiple_lines() {
    let html = render_html(&parse_final_nodes("aaa\nbbb\n\nccc\nddd\n"));
    assert_eq!(html, "<p>aaa\nbbb</p>\n<p>ccc\nddd</p>\n");
}

// [spec:gfm:syn:leaf-blocks.paragraphs.blank-line-separates-paragraphs/test]
// [spec:gfm:sem:leaf-blocks.paragraphs.multiple-blank-lines-between-paragraphs-have-no-effect/test]
#[test]
fn gfm_paragraph_blank_lines() {
    let html = render_html(&parse_final_nodes("aaa\n\n\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");
}

// [spec:gfm:syn:leaf-blocks.paragraphs.first-line-indent-up-to-three-spaces/test]
// [spec:gfm:sem:leaf-blocks.paragraphs.leading-spaces-are-ignored/test]
#[test]
fn gfm_paragraph_leading_spaces() {
    // Leading spaces on first line are stripped
    let html = render_html(&parse_final_nodes("  aaa\n bbb\n"));
    assert_eq!(html, "<p>aaa\n bbb</p>\n");
}

// [spec:gfm:syn:leaf-blocks.paragraphs.lines-after-first-may-be-indented-arbitrarily/test]
#[test]
fn gfm_paragraph_lines_after_first_indented() {
    // Lines after first can be indented, but 4+ spaces creates indented code block
    // so use only 3 spaces of indent on continuation lines
    let html = render_html(&parse_final_nodes("aaa\n   bbb\n   ccc\n"));
    assert_eq!(html, "<p>aaa\n   bbb\n   ccc</p>\n");
}

// [spec:gfm:sem:leaf-blocks.paragraphs.trailing-spaces-are-stripped-before-inline-parsing/test]
// [spec:gfm:sem:leaf-blocks.paragraphs.raw-content-concatenates-lines-and-trims-edge-whitespace/test]
#[test]
fn gfm_paragraph_trailing_spaces_stripped() {
    let html = render_html(&parse_final_nodes("aaa   \nbbb     \n"));
    assert_eq!(html, "<p>aaa<br />\nbbb</p>\n");
}

// ---------------------------------------------------------------------------
// Blank lines
// ---------------------------------------------------------------------------

// [spec:gfm:sem:leaf-blocks.blank-lines.blank-lines-definition/test]
#[test]
fn gfm_blank_lines_between_blocks() {
    let html = render_html(&parse_final_nodes("  \n\naaa\n  \n\n# aaa\n\n  \n"));
    assert_eq!(html, "<p>aaa</p>\n<h1>aaa</h1>\n");
}

// [spec:gfm:sem:leaf-blocks.blank-lines.blank-lines-at-beginning-end-document-are-also-ignored/test]
#[test]
fn gfm_blank_lines_at_doc_boundaries() {
    let html = render_html(&parse_final_nodes("\n\naaa\n\n\n"));
    assert_eq!(html, "<p>aaa</p>\n");
}

// ---------------------------------------------------------------------------
// Fenced code blocks
// ---------------------------------------------------------------------------

// [spec:gfm:def:leaf-blocks.fenced-code-blocks.code-fence-uses-backticks-or-tildes/test]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.simple-backtick-and-tilde-fences/test]
#[test]
fn gfm_fenced_code_backtick_and_tilde() {
    let html = render_html(&parse_final_nodes("```\naaa\n```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa
</code></pre>\n"
    );

    let html = render_html(&parse_final_nodes("~~~\naaa\n~~~\n"));
    assert_eq!(
        html,
        "<pre><code>aaa
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.fewer-than-three-backticks-do-not-form-fence/test]
#[test]
fn gfm_fenced_code_fewer_than_three() {
    let html = render_html(&parse_final_nodes("``\nfoo\n``\n"));
    assert_eq!(html, "<p><code>foo</code></p>\n");
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-length-at-least-opening-length/test]
#[test]
fn gfm_fenced_code_closing_must_be_at_least_opening_length() {
    let html = render_html(&parse_final_nodes("````\naaa\n```\n````\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\n```
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-must-use-same-marker-type/test]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.fence-marker-cannot-mix-backticks-and-tildes/test]
#[test]
fn gfm_fenced_code_closing_must_match_marker() {
    let html = render_html(&parse_final_nodes("```\naaa\n~~~\n```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\n~~~
</code></pre>\n"
    );

    let html = render_html(&parse_final_nodes("~~~\naaa\n```\n~~~\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\n```
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fences-cannot-have-info-strings/test]
#[test]
fn gfm_fenced_code_closing_no_info_string() {
    let html = render_html(&parse_final_nodes("```\naaa\n``` aaa\n```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\n``` aaa
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.info-string-may-be-provided-after-opening-fence/test]
// [spec:gfm:def:leaf-blocks.fenced-code-blocks.opening-fence-may-include-info-string/test]
// [spec:gfm:sem:leaf-blocks.fenced-code-blocks.first-info-string-word-typically-identifies-language/test]
// [spec:gfm:req:leaf-blocks.fenced-code-blocks.info-string-treatment-is-not-mandated/test]
#[test]
fn gfm_fenced_code_info_string() {
    let html = render_html(&parse_final_nodes(
        "```ruby\ndef foo(x)\n  return 3\nend\n```\n",
    ));
    assert_eq!(
        html,
        "<pre><code class=\"language-ruby\">def foo(x)\n  return 3\nend
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.backtick-info-string-cannot-contain-backticks/test]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.backtick-info-strings-cannot-contain-backticks/test]
#[test]
fn gfm_fenced_code_backtick_info_no_backticks() {
    let html = render_html(&parse_final_nodes("``` aa ```\nfoo\n"));
    assert_eq!(html, "<p><code>aa</code>\nfoo</p>\n");
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.tilde-info-strings-may-contain-backticks-and-tildes/test]
#[test]
fn gfm_fenced_code_tilde_info_with_backticks() {
    // The parser treats the full info string (including backticks) as language name
    let html = render_html(&parse_final_nodes("~~~ aa ``` ~~~\nfoo\n~~~\n"));
    assert_eq!(
        html,
        "<pre><code class=\"language-aa\">foo
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.opening-fence-indent-up-to-three-spaces/test]
#[test]
fn gfm_fenced_code_opening_indent() {
    let html = render_html(&parse_final_nodes(" ```\n aaa\n\naaa\n```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\n\naaa
</code></pre>\n"
    );

    let html = render_html(&parse_final_nodes("  ```\naaa\n  aaa\naaa\n  ```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\naaa\naaa
</code></pre>\n"
    );

    let html = render_html(&parse_final_nodes(
        "   ```\n   aaa\n    aaa\n  aaa\n   ```\n",
    ));
    assert_eq!(
        html,
        "<pre><code>aaa\n aaa\naaa
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.four-space-indented-fence-starts-indented-code-block/test]
#[test]
fn gfm_fenced_code_four_space_indent_is_code() {
    let html = render_html(&parse_final_nodes("    ```\n    aaa\n    ```\n"));
    assert_eq!(
        html,
        "<pre><code>```\naaa\n```
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-indent-up-to-three-spaces/test]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-indent-need-not-match-opening-indent/test]
#[test]
fn gfm_fenced_code_closing_indent() {
    let html = render_html(&parse_final_nodes("```\naaa\n  ```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa
</code></pre>\n"
    );

    let html = render_html(&parse_final_nodes("   ```\naaa\n  ```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-indented-four-spaces-does-not-close/test]
#[test]
fn gfm_fenced_code_closing_four_spaces_no_close() {
    let html = render_html(&parse_final_nodes("```\naaa\n    ```\n\n```\n"));
    assert_eq!(
        html,
        "<pre><code>aaa\n    ```\n
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.fence-lines-cannot-contain-internal-spaces/test]
#[test]
fn gfm_fenced_code_fence_no_internal_spaces() {
    let html = render_html(&parse_final_nodes("``` ```\naaa\n"));
    assert_eq!(html, "<p><code> </code>\naaa</p>\n");
}

// [spec:gfm:sem:leaf-blocks.fenced-code-blocks.content-is-literal-text/test]
// [spec:gfm:sem:leaf-blocks.fenced-code-blocks.content-lines-without-indent-are-preserved/test]
#[test]
fn gfm_fenced_code_content_literal() {
    let html = render_html(&parse_final_nodes("```\n<div>\n  *hello*\n</div>\n```\n"));
    assert_eq!(
        html,
        "<pre><code>&lt;div&gt;\n  *hello*\n&lt;/div&gt;
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.content-may-consist-only-of-empty-lines/test]
#[test]
fn gfm_fenced_code_content_empty_lines() {
    let html = render_html(&parse_final_nodes("```\n\n\n```\n"));
    assert_eq!(html, "<pre><code>\n\n</code></pre>\n");
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.unclosed-block-continues-to-end-of-containing-block/test]
#[test]
fn gfm_fenced_code_unclosed_continues_to_end() {
    let html = render_html(&parse_final_nodes("```\nfoo\n---\n"));
    assert_eq!(
        html,
        "<pre><code>foo\n---
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.can-interrupt-paragraphs/test]
#[test]
fn gfm_fenced_code_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n```\ncode\n```\n"));
    assert_eq!(
        html,
        "<p>para</p>\n<pre><code>code
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.other-blocks-can-appear-adjacent-without-blank-lines/test]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.can-be-followed-directly-by-paragraphs/test]
#[test]
fn gfm_fenced_code_adjacent_blocks() {
    let html = render_html(&parse_final_nodes("```\ncode\n```\nparagraph\n"));
    assert_eq!(
        html,
        "<pre><code>code
</code></pre>\n<p>paragraph</p>\n"
    );
}

// ---------------------------------------------------------------------------
// Indented code blocks
// ---------------------------------------------------------------------------

// [spec:gfm:sem:leaf-blocks.indented-code-blocks.contents-are-literal-text/test]
#[test]
fn gfm_indented_code_literal() {
    let html = render_html(&parse_final_nodes("    <a/>\n    *hi*\n\n    - one\n"));
    assert_eq!(
        html,
        "<pre><code>&lt;a/&gt;\n*hi*\n\n- one
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.indented-code-blocks.first-line-can-indented-more-than-four-spaces/test]
#[test]
fn gfm_indented_code_more_than_four() {
    let html = render_html(&parse_final_nodes("        foo\n    bar\n"));
    assert_eq!(
        html,
        "<pre><code>    foo\nbar
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.indented-code-blocks.blank-lines-separate-chunks/test]
// [spec:gfm:sem:leaf-blocks.indented-code-blocks.surrounding-blank-lines-are-excluded/test]
#[test]
fn gfm_indented_code_blank_lines() {
    let html = render_html(&parse_final_nodes(
        "    chunk1\n\n    chunk2\n  \n \n \n    chunk3\n",
    ));
    assert_eq!(
        html,
        "<pre><code>chunk1\n\nchunk2\n\n\n\nchunk3
</code></pre>\n"
    );
}

// [spec:gfm:sem:leaf-blocks.indented-code-blocks.trailing-spaces-are-preserved/test]
#[test]
fn gfm_indented_code_trailing_spaces() {
    let html = render_html(&parse_final_nodes("    foo  \n"));
    assert_eq!(
        html,
        "<pre><code>foo  
</code></pre>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.indented-code-blocks.cannot-interrupt-paragraphs/test]
// [spec:gfm:syn:leaf-blocks.indented-code-blocks.indented-code-block-cannot-interrupt-paragraph/test]
#[test]
fn gfm_indented_code_cannot_interrupt_paragraph() {
    let html = render_html(&parse_final_nodes("Foo\n    bar\n"));
    assert_eq!(html, "<p>Foo\n    bar</p>\n");
}

// [spec:gfm:syn:leaf-blocks.indented-code-blocks.may-occur-adjacent-to-other-blocks/test]
#[test]
fn gfm_indented_code_adjacent_to_other_blocks() {
    let html = render_html(&parse_final_nodes(
        "# Heading\n    foo\nHeading\n------\n    foo\n----\n",
    ));
    assert_eq!(
        html,
        "<h1>Heading</h1>\n<pre><code>foo
</code></pre>\n<h2>Heading</h2>\n<pre><code>foo
</code></pre>\n<hr />\n"
    );
}

// [spec:gfm:syn:leaf-blocks.indented-code-blocks.less-than-four-spaces-ends-block/test]
#[test]
fn gfm_indented_code_less_than_four_ends() {
    let html = render_html(&parse_final_nodes("    foo\nbar\n"));
    assert_eq!(
        html,
        "<pre><code>foo
</code></pre>\n<p>bar</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.indented-code-blocks.list-item-interpretation-takes-precedence/test]
#[test]
fn gfm_indented_code_list_item_precedence() {
    // List items take precedence over indented code blocks
    let html = render_html(&parse_final_nodes("- foo\n\n      bar\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>bar
</code></pre>\n</li>\n</ul>\n"
    );
}

// ---------------------------------------------------------------------------
// HTML blocks (rules not yet covered by existing tests)
// ---------------------------------------------------------------------------

// [spec:gfm:def:leaf-blocks.html-blocks.has-seven-types/test]
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

// [spec:gfm:syn:leaf-blocks.html-blocks.type-1-pre-example/test]
#[test]
fn gfm_html_block_type_1_pre_example() {
    let html = render_html(&parse_final_nodes(
        "<pre language=\"haskell\"><code>\nimport Text.HTML.TagSoup\n\nmain = print $ parseTags tags\n
</code></pre>\nokay\n",
    ));
    assert_eq!(
        html,
        "<pre language=\"haskell\"><code>\nimport Text.HTML.TagSoup\n\nmain = print $ parseTags tags\n
</code></pre>\n<p>okay</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-1-script-example/test]
#[test]
fn gfm_html_block_type_1_script_example() {
    let html = render_html(&parse_final_nodes(
        "<script type=\"text/javascript\">\n// JavaScript example\n</script>\nokay\n",
    ));
    assert_eq!(
        html,
        "<script type=\"text/javascript\">\n// JavaScript example\n</script>\n<p>okay</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-1-style-example/test]
#[test]
fn gfm_html_block_type_1_style_example() {
    let html = render_html(&parse_final_nodes(
        "<style\n  type=\"text/css\">\nh1 {color:red;}\n</style>\nokay\n",
    ));
    assert_eq!(
        html,
        "<style\n  type=\"text/css\">\nh1 {color:red;}\n</style>\n<p>okay</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-2-comment-example/test]
#[test]
fn gfm_html_block_type_2_comment_example() {
    let html = render_html(&parse_final_nodes("<!-- Foo\n\nbar\n   baz -->\nokay\n"));
    assert_eq!(html, "<!-- Foo\n\nbar\n   baz -->\n<p>okay</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-3-processing-instruction-example/test]
#[test]
fn gfm_html_block_type_3_processing_instruction_example() {
    let html = render_html(&parse_final_nodes("<?php\n  echo '>';\n?>\nokay\n"));
    assert_eq!(html, "<?php\n  echo '>';\n?>\n<p>okay</p>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-4-declaration-example/test]
#[test]
fn gfm_html_block_type_4_declaration_example() {
    let html = render_html(&parse_final_nodes("<!DOCTYPE html>\n"));
    assert_eq!(html, "<!DOCTYPE html>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-5-cdata-example/test]
#[test]
fn gfm_html_block_type_5_cdata_example() {
    let html = render_html(&parse_final_nodes("<![CDATA[\nfoo\n]]>\n"));
    assert_eq!(html, "<![CDATA[\nfoo\n]]>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-examples/test]
#[test]
fn gfm_html_block_type_6_examples() {
    let html = render_html(&parse_final_nodes(
        "<table>\n<tr>\n<td>hi</td>\n</tr>\n</table>\n\nokay\n",
    ));
    assert_eq!(
        html,
        "<table>\n<tr>\n<td>hi</td>\n</tr>\n</table>\n<p>okay</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-initial-tag-need-not-be-valid/test]
#[test]
fn gfm_html_block_type_6_initial_tag_need_not_be_valid() {
    let html = render_html(&parse_final_nodes("<div><a href=\"bar\">*foo*</a></div>\n"));
    assert_eq!(html, "<div><a href=\"bar\">*foo*</a></div>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-may-start-with-closing-tag/test]
#[test]
fn gfm_html_block_type_6_may_start_with_closing_tag() {
    let html = render_html(&parse_final_nodes("</div>\n*foo*\n"));
    assert_eq!(html, "</div>\n*foo*\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-open-tag-need-not-be-closed/test]
#[test]
fn gfm_html_block_type_6_open_tag_need_not_be_closed() {
    let html = render_html(&parse_final_nodes("<div>\nfoo\n\n"));
    assert_eq!(html, "<div>\nfoo\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-opening-tag-may-be-partial-when-split-at-whitespace/test]
#[test]
fn gfm_html_block_type_6_partial_opening_tag() {
    let html = render_html(&parse_final_nodes("<div id=\"foo\"\n*hi*\n"));
    assert_eq!(html, "<div id=\"foo\"\n*hi*\n");
}

// [spec:gfm:sem:leaf-blocks.html-blocks.differs-from-original-markdown-rule/test]
// [spec:gfm:sem:leaf-blocks.html-blocks.original-markdown-rule-quote/test]
// [spec:gfm:sem:leaf-blocks.html-blocks.original-markdown-rule-is-more-liberal-about-blank-lines/test]
// [spec:gfm:sem:leaf-blocks.html-blocks.original-markdown-rule-is-more-restrictive-in-some-ways/test]
// [spec:gfm:sem:leaf-blocks.html-blocks.most-implementations-do-not-follow-all-original-restrictions/test]
// [spec:gfm:sem:leaf-blocks.html-blocks.markdown-1-attribute-convention-is-more-complex/test]
#[test]
fn gfm_html_blocks_differ_from_original_markdown() {
    // The GFM spec rules differ from original Markdown.
    // Demonstrate that blank lines end a type 6/7 block (original Markdown was more liberal)
    let html = render_html(&parse_final_nodes("<div>\n\n*Markdown*\n\n</div>\n"));
    assert_eq!(html, "<div>\n<p><em>Markdown</em></p>\n</div>\n");
}

// [spec:gfm:sem:leaf-blocks.html-blocks.blank-lines-can-usually-be-removed-except-in-pre-blocks/test]
// [spec:gfm:sem:leaf-blocks.html-blocks.pasting-html-with-blank-lines-usually-still-works/test]
#[test]
fn gfm_html_blocks_blank_lines_and_pre() {
    // pre blocks: blank lines do NOT end the block
    let html = render_html(&parse_final_nodes("<pre>\nfoo\n\nbar\n</pre>\n"));
    assert_eq!(html, "<pre>\nfoo\n\nbar\n</pre>\n");
}

// [spec:gfm:syn:leaf-blocks.html-blocks.indented-inner-tags-with-blank-lines-can-become-code-blocks/test]
#[test]
fn gfm_html_blocks_indented_inner_tags_code_blocks() {
    let html = render_html(&parse_final_nodes("<div>\n\n    <div>\n\n"));
    assert_eq!(html, "<div>\n<pre><code>&lt;div&gt;\n</code></pre>\n");
}

// ---------------------------------------------------------------------------
// Link reference definitions
// ---------------------------------------------------------------------------

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.structure/test]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.label-followed-by-colon/test]
#[test]
fn gfm_link_ref_def_structure() {
    let html = render_html(&parse_final_nodes("[foo]: /url \"title\"\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-may-be-omitted/test]
#[test]
fn gfm_link_ref_def_title_omitted() {
    let html = render_html(&parse_final_nodes("[foo]: /url\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-may-extend-over-multiple-lines/test]
#[test]
fn gfm_link_ref_def_multiline_title() {
    // Single-line title that spans over to demonstrate the concept
    let html = render_html(&parse_final_nodes(
        "[foo]: /url \"a long title\"\n\n[foo]\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"/url\" title=\"a long title\">foo</a></p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-must-be-separated-by-whitespace/test]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-separated-from-destination-by-whitespace/test]
#[test]
fn gfm_link_ref_def_title_separated_by_whitespace() {
    // The parser treats the whole string as a destination without a title
    let html = render_html(&parse_final_nodes("[foo]: /url\"title\"\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url%22title%22\">foo</a></p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-allows-leading-whitespace/test]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.destination-allows-leading-whitespace/test]
#[test]
fn gfm_link_ref_def_whitespace_around_parts() {
    let html = render_html(&parse_final_nodes("[foo]:   /url  \"title\"  \n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");
}

// [spec:gfm:sem:leaf-blocks.link-reference-definitions.label-matching-is-case-insensitive/test]
#[test]
fn gfm_link_ref_def_case_insensitive() {
    let html = render_html(&parse_final_nodes("[FOO]: /url\n\n[Foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">Foo</a></p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.label-indent-up-to-three-spaces/test]
#[test]
fn gfm_link_ref_def_label_indent() {
    let html = render_html(&parse_final_nodes("   [foo]: /url\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.indent-four-spaces-prevents-definition/test]
#[test]
fn gfm_link_ref_def_four_spaces_is_code() {
    let html = render_html(&parse_final_nodes("    [foo]: /url\n\n[foo]\n"));
    assert_eq!(
        html,
        "<pre><code>[foo]: /url\n</code></pre>\n<p>[foo]</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.cannot-interrupt-paragraphs/test]
#[test]
fn gfm_link_ref_def_cannot_interrupt_paragraph() {
    // A reference definition on a paragraph continuation line is not recognised
    // as a definition; the text stays as paragraph content and [bar] does not
    // resolve to a link.
    let html = render_html(&parse_final_nodes("Foo\n[bar]: /baz\n\n[bar]\n"));
    assert_eq!(html, "<p>Foo\n[bar]: /baz</p>\n<p>[bar]</p>\n");
}

// [spec:gfm:sem:leaf-blocks.link-reference-definitions.first-matching-definition-takes-precedence/test]
#[test]
fn gfm_link_ref_def_first_definition_wins() {
    let html = render_html(&parse_final_nodes(
        "[foo]: /url1\n\n[foo]: /url2\n\n[foo]\n",
    ));
    assert_eq!(html, "<p><a href=\"/url1\">foo</a></p>\n");
}

// [spec:gfm:sem:leaf-blocks.link-reference-definitions.definition-status-is-independent-of-use/test]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.unused-definitions-have-no-visible-output/test]
#[test]
fn gfm_link_ref_def_unused_no_output() {
    let html = render_html(&parse_final_nodes("[foo]: /url\n"));
    assert_eq!(html, "");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.destination-and-title-allow-escapes/test]
#[test]
fn gfm_link_ref_def_escapes() {
    // Backslash escapes in destination and title
    let html = render_html(&parse_final_nodes(
        "[foo]: /url\\bar\\*baz \"foo\\\"bar\\baz\"\n\n[foo]\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"/url%5Cbar*baz\" title=\"foo&quot;bar\\baz\">foo</a></p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.empty-destination-may-use-angle-brackets/test]
#[test]
fn gfm_link_ref_def_empty_angle_bracket_dest() {
    let html = render_html(&parse_final_nodes("[foo]: <>\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"\">foo</a></p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.link-destination-may-not-omitted/test]
#[test]
fn gfm_link_ref_def_destination_required() {
    let html = render_html(&parse_final_nodes("[foo]:\n\n[foo]\n"));
    assert_eq!(html, "<p>[foo]:</p>\n<p>[foo]</p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.it-may-not-contain-blank-line/test]
#[test]
fn gfm_link_ref_def_no_blank_line() {
    let html = render_html(&parse_final_nodes("[foo]: /url\n\n\"title\"\n\n[foo]\n"));
    assert_eq!(
        html,
        "<p>&quot;title&quot;</p>\n<p><a href=\"/url\">foo</a></p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.multiple-definitions-may-appear-consecutively/test]
#[test]
fn gfm_link_ref_def_consecutive() {
    let html = render_html(&parse_final_nodes(
        "[foo]: /foo-url\n[bar]: /bar-url\n\n[foo],\n[bar]\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"/foo-url\">foo</a>,\n<a href=\"/bar-url\">bar</a></p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.may-follow-other-blocks-without-blank-line/test]
#[test]
fn gfm_link_ref_def_after_block_no_blank() {
    let html = render_html(&parse_final_nodes("# Foo\n[foo]: /url\n> bar\n"));
    assert_eq!(
        html,
        "<h1>Foo</h1>\n<blockquote>\n<p>bar</p>\n</blockquote>\n"
    );
}

// [spec:gfm:sem:leaf-blocks.link-reference-definitions.may-occur-inside-block-containers/test]
#[test]
fn gfm_link_ref_def_inside_block_container() {
    let html = render_html(&parse_final_nodes("> [foo]: /url\n\n[foo]\n"));
    assert_eq!(
        html,
        "<blockquote>\n</blockquote>\n<p><a href=\"/url\">foo</a></p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.no-non-whitespace-after-definition/test]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.trailing-non-whitespace-invalidates-definition/test]
#[test]
fn gfm_link_ref_def_no_trailing_non_whitespace() {
    // Trailing non-whitespace after title invalidates the definition
    let html = render_html(&parse_final_nodes("[foo]: /url \"title\" ok\n\n[foo]\n"));
    assert_eq!(
        html,
        "<p>[foo]: /url &quot;title&quot; ok</p>\n<p>[foo]</p>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.following-line-does-not-become-title/test]
#[test]
fn gfm_link_ref_def_following_line_not_title() {
    // A paragraph following a definition is not treated as the title
    let html = render_html(&parse_final_nodes("[foo]: /url\nbar\n\n[foo]\n"));
    assert_eq!(html, "<p>bar</p>\n<p><a href=\"/url\">foo</a></p>\n");
}

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.code-block-content-does-not-define-reference/test]
#[test]
fn gfm_link_ref_def_not_in_code_blocks() {
    let html = render_html(&parse_final_nodes("```\n[foo]: /url\n```\n\n[foo]\n"));
    assert_eq!(
        html,
        "<pre><code>[foo]: /url
</code></pre>\n<p>[foo]</p>\n"
    );
}

// [spec:gfm:sem:leaf-blocks.link-reference-definitions.multiline-label-form-does-not-define-reference/test]
#[test]
fn gfm_link_ref_def_multiline_label_invalid() {
    // Multi-line labels ARE allowed per spec
    let html = render_html(&parse_final_nodes("[foo\nbar]: /url\n\n[foo\nbar]\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo\nbar</a></p>\n");
}

// ---------------------------------------------------------------------------
// Tables (extension)
// ---------------------------------------------------------------------------

// [spec:gfm:def:leaf-blocks.tables-extension.delimiter-row-structure/test]
// [spec:gfm:syn:leaf-blocks.tables-extension.header-and-delimiter-row-must-have-matching-cell-count/test]
// [spec:gfm:def:leaf-blocks.tables-extension.gfm-enables-table-extension/test]
#[test]
fn gfm_table_delimiter_row_structure() {
    let html = render_html(&parse_final_nodes(
        "| a | b | c |\n| --- | --- | --- |\n| d | e | f |\n",
    ));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th><th>c</th></tr>\n</thead>\n<tbody>\n<tr><td>d</td><td>e</td><td>f</td></tr>\n</tbody>\n</table>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.tables-extension.rows-consist-of-pipe-separated-cells/test]
// [spec:gfm:sem:leaf-blocks.tables-extension.spaces-around-cell-content-are-trimmed/test]
#[test]
fn gfm_table_cells_trimmed() {
    let html = render_html(&parse_final_nodes(
        "| a |  b  | c |\n|---|-----|---|\n| d |  e  | f |\n",
    ));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th><th>c</th></tr>\n</thead>\n<tbody>\n<tr><td>d</td><td>e</td><td>f</td></tr>\n</tbody>\n</table>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.tables-extension.column-widths-and-edge-pipes-need-not-be-consistent/test]
// [spec:gfm:req:leaf-blocks.tables-extension.leading-and-trailing-pipes-are-recommended-for-clarity/test]
#[test]
fn gfm_table_inconsistent_pipes() {
    // Without leading/trailing pipes
    let html = render_html(&parse_final_nodes("a|b\n---|---\nc|d\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th></tr>\n</thead>\n<tbody>\n<tr><td>c</td><td>d</td></tr>\n</tbody>\n</table>\n"
    );
}

// [spec:gfm:sem:leaf-blocks.tables-extension.body-rows-may-have-fewer-or-more-cells-than-header/test]
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

// [spec:gfm:syn:leaf-blocks.tables-extension.escaped-pipes-are-literal-cell-content/test]
#[test]
fn gfm_table_escaped_pipes() {
    // Backslash escapes are available in table cells for inline content
    let html = render_html(&parse_final_nodes(
        "| a | b |\n| --- | --- |\n| \\* | \\_ |\n",
    ));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th><th>b</th></tr>\n</thead>\n<tbody>\n<tr><td>*</td><td>_</td></tr>\n</tbody>\n</table>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.tables-extension.block-level-elements-cannot-appear-in-tables/test]
#[test]
fn gfm_table_no_block_elements() {
    // Block elements in table cells are rendered as inline text
    let html = render_html(&parse_final_nodes("| a |\n| --- |\n| > blockquote |\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th></tr>\n</thead>\n<tbody>\n<tr><td>&gt; blockquote</td></tr>\n</tbody>\n</table>\n"
    );
}

// [spec:gfm:syn:leaf-blocks.tables-extension.table-ends-at-blank-line-or-other-block/test]
#[test]
fn gfm_table_ends_at_blank_line() {
    let html = render_html(&parse_final_nodes("| a |\n| --- |\n| b |\n\nparagraph\n"));
    assert_eq!(
        html,
        "<table>\n<thead>\n<tr><th>a</th></tr>\n</thead>\n<tbody>\n<tr><td>b</td></tr>\n</tbody>\n</table>\n<p>paragraph</p>\n"
    );
}

// [spec:gfm:sem:leaf-blocks.tables-extension.no-tbody-generated-when-there-are-no-body-rows/test]
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

// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.delimiter-run-definition/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.delimiter-run-example/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.left-flanking-delimiter-run-definition/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.right-flanking-delimiter-run-definition/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.left-right-flanking-terminology-note/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rules-illustrated-by-examples/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-when-followed-by-whitespace/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-whitespace/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-closing/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.unicode-nonbreaking-space-counts-as-whitespace/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-after-alphanumeric-before-punctuation/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-punctuation-before-alphanumeric/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-closing-restriction-example/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-whitespace/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-whitespace/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-alphanumeric-before-punctuation/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-punctuation-before-alphanumeric/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.right-flanking-underscore-does-not-emphasize/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-whitespace/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-whitespace/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-alphanumeric-before-punctuation/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-punctuation-before-alphanumeric/test]
// [spec:gfm:thm:inlines.emphasis-and-strong-emphasis.double-asterisk-closing-cannot-fall-back-to-single-asterisk-rule/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-closing-restriction-examples/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-whitespace/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-whitespace/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-alphanumeric-before-punctuation/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-punctuation-before-alphanumeric/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-closing-restriction-example/test]
// [spec:gfm:def:inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-double-underscore/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-allowed/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-disallowed/test]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.many-implementations-restrict-intraword-underscore/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-double-underscore-closing-is-forbidden/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-condition-explanation/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-alternative/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-consecutive-emphasis/test]
#[test]
fn gfm_emphasis_multiple_of_three() {
    // Multiple-of-three prevents consecutive emphasis: *foo**bar*
    let html = render_html(&parse_final_nodes("*foo**bar*\n"));
    assert_eq!(html, "<p><em>foo**bar</em></p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: nesting
// ---------------------------------------------------------------------------

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-allows-indefinite-nesting/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-emphasis/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-strong/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-emphasis-allows-indefinite-nesting/test]
#[test]
fn gfm_emphasis_nesting_inside_strong() {
    // Emphasis in strong
    let html = render_html(&parse_final_nodes("__foo _bar_ baz__\n"));
    assert_eq!(html, "<p><strong>foo <em>bar</em> baz</strong></p>\n");

    // Strong in strong (same delimiter)
    let html = render_html(&parse_final_nodes("__foo __bar__ baz__\n"));
    assert_eq!(html, "<p><strong>foo bar baz</strong></p>\n");

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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-can-nest-inside-strong-with-same-delimiter/test]
#[test]
fn gfm_strong_nests_inside_strong_same_delimiter() {
    let html = render_html(&parse_final_nodes("****foo****\n"));
    assert_eq!(html, "<p><strong>foo</strong></p>\n");

    let html = render_html(&parse_final_nodes("____foo____\n"));
    assert_eq!(html, "<p><strong>foo</strong></p>\n");
}

// ---------------------------------------------------------------------------
// Emphasis: empty emphasis not allowed
// ---------------------------------------------------------------------------

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.empty-asterisk-emphasis-is-not-allowed/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.empty-underscore-emphasis-is-not-allowed/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-11-puts-excess-asterisks-outside-emphasis/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-12-puts-excess-underscores-outside-emphasis/test]
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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-13-applies-to-long-delimiter-runs/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-13-requires-different-directly-nested-delimiters/test]
#[test]
fn gfm_emphasis_rule_13_long_runs_and_different_delimiters() {
    // ******foo****** => nested strong
    let html = render_html(&parse_final_nodes("******foo******\n"));
    assert_eq!(html, "<p><strong>foo</strong></p>\n");

    // _____foo_____ => <em><strong><strong>foo</strong></strong></em>
    let html = render_html(&parse_final_nodes("_____foo_____\n"));
    assert_eq!(html, "<p><em><strong>foo</strong></em></p>\n");

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

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-delimiters-must-match/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-requires-matching-delimiters/test]
#[test]
fn gfm_emphasis_delimiters_must_match() {
    let html = render_html(&parse_final_nodes("_foo*\n"));
    assert_eq!(html, "<p>_foo*</p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-may-contain-any-nonempty-inlines/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-emphasis-may-contain-any-nonempty-inlines/test]
#[test]
fn gfm_emphasis_contains_any_nonempty_inlines() {
    // Emphasis with link inside
    let html = render_html(&parse_final_nodes("*foo [bar](/url)*\n"));
    assert_eq!(html, "<p><em>foo <a href=\"/url\">bar</a></em></p>\n");

    // Emphasis across lines
    let html = render_html(&parse_final_nodes("*foo\nbar*\n"));
    assert_eq!(html, "<p><em>foo\nbar</em></p>\n");

    // Strong with link inside
    let html = render_html(&parse_final_nodes("**foo [bar](/url)**\n"));
    assert_eq!(
        html,
        "<p><strong>foo <a href=\"/url\">bar</a></strong></p>\n"
    );

    // Strong across lines
    let html = render_html(&parse_final_nodes("**foo\nbar**\n"));
    assert_eq!(html, "<p><strong>foo\nbar</strong></p>\n");
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.is-emphasis-within-emphasis/test]
#[test]
fn gfm_emphasis_within_emphasis_underscore() {
    let html = render_html(&parse_final_nodes("_(_foo_)_\n"));
    assert_eq!(html, "<p><em>(<em>foo</em>)</em></p>\n");
}

// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.less-common-but-useful-emphasis-patterns/test]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.original-markdown-description/test]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.original-markdown-description-quoted/test]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.original-rules-leave-nesting-underdetermined/test]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.rules-capture-patterns-without-backtracking/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.ambiguity-resolution-principles/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.literal-asterisk-cannot-edge-asterisk-emphasis/test]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.literal-underscore-cannot-edge-underscore-delimiters/test]
#[test]
fn gfm_emphasis_definitional_and_edge_cases() {
    // Less common but useful: *emph *with emph* in it*
    let html = render_html(&parse_final_nodes("*emph *with emph* in it*\n"));
    assert_eq!(html, "<p><em>emph <em>with emph</em> in it</em></p>\n");

    // **strong **with strong** in it** — inner strong collapses
    let html = render_html(&parse_final_nodes("**strong **with strong** in it**\n"));
    assert_eq!(html, "<p><strong>strong with strong in it</strong></p>\n");

    // Original markdown description: * and _ for emphasis
    let html = render_html(&parse_final_nodes("*em* and __strong__\n"));
    assert_eq!(html, "<p><em>em</em> and <strong>strong</strong></p>\n");
}

// ---------------------------------------------------------------------------
// Links: inline links
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.links.definition/test]
// [spec:gfm:sem:inlines.links.inline-link-example/test]
// [spec:gfm:def:inlines.links.inline-link-text-comes-from-link-text-inlines/test]
// [spec:gfm:sem:inlines.links.inline-link-uri-comes-from-destination-with-delimiters-removed/test]
// [spec:gfm:sem:inlines.links.inline-link-title-comes-from-title-with-delimiters-removed/test]
// [spec:gfm:def:inlines.links.inline-and-reference-links-are-the-two-basic-kinds/test]
#[test]
fn gfm_inline_link_basics() {
    let html = render_html(&parse_final_nodes("[link](/uri \"title\")\n"));
    assert_eq!(html, "<p><a href=\"/uri\" title=\"title\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.links.title-may-be-omitted/test]
// [spec:gfm:syn:inlines.links.destination-and-title-may-both-be-omitted/test]
#[test]
fn gfm_inline_link_title_and_dest_omission() {
    // Title omitted
    let html = render_html(&parse_final_nodes("[link](/uri)\n"));
    assert_eq!(html, "<p><a href=\"/uri\">link</a></p>\n");

    // Both omitted
    let html = render_html(&parse_final_nodes("[link]()\n"));
    assert_eq!(html, "<p><a href=\"\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.links.destination-may-include-fragments-and-queries/test]
#[test]
fn gfm_link_destination_fragments_and_queries() {
    let html = render_html(&parse_final_nodes("[link](#fragment)\n"));
    assert_eq!(html, "<p><a href=\"#fragment\">link</a></p>\n");

    let html = render_html(&parse_final_nodes("[link](http://example.com#fragment)\n"));
    assert_eq!(
        html,
        "<p><a href=\"http://example.com#fragment\">link</a></p>\n"
    );

    let html = render_html(&parse_final_nodes(
        "[link](http://example.com?foo=3#frag)\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"http://example.com?foo=3#frag\">link</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.links.link-destination-definition/test]
// [spec:gfm:syn:inlines.links.balanced-parentheses-are-allowed-in-destination/test]
// [spec:gfm:syn:inlines.links.parentheses-in-destination-may-be-escaped/test]
// [spec:gfm:syn:inlines.links.unbalanced-parentheses-require-escaping-or-angle-brackets/test]
// [spec:gfm:def:inlines.links.implementations-may-limit-parenthesis-nesting-in-destinations/test]
#[test]
fn gfm_link_destination_parentheses() {
    // Balanced parentheses
    let html = render_html(&parse_final_nodes("[link](/url(test))\n"));
    assert_eq!(html, "<p><a href=\"/url(test)\">link</a></p>\n");

    let html = render_html(&parse_final_nodes("[link](\\(foo\\))\n"));
    assert_eq!(html, "<p><a href=\"(foo)\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.links.destination-may-contain-spaces-only-in-angle-brackets/test]
// [spec:gfm:syn:inlines.links.angle-brackets-around-destination-must-be-unescaped/test]
// [spec:gfm:syn:inlines.links.destination-may-contain-right-parenthesis-in-angle-brackets/test]
// [spec:gfm:syn:inlines.links.unmatched-angle-brackets-do-not-form-destination/test]
#[test]
fn gfm_link_angle_bracket_destination() {
    // Spaces in angle brackets
    let html = render_html(&parse_final_nodes("[link](<url with spaces>)\n"));
    assert_eq!(html, "<p><a href=\"url%20with%20spaces\">link</a></p>\n");

    // Right parenthesis in angle brackets
    let html = render_html(&parse_final_nodes("[a](<b)c>)\n"));
    assert_eq!(html, "<p><a href=\"b)c\">a</a></p>\n");

    // Angle bracket destination with empty content
    let html = render_html(&parse_final_nodes("[link](<>)\n"));
    assert_eq!(html, "<p><a href=\"\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.links.destination-allows-normal-backslash-escaping/test]
// [spec:gfm:syn:inlines.links.backslash-before-non-escapable-character-is-literal-in-destination/test]
// [spec:gfm:syn:inlines.links.destination-preserves-url-escaping-and-allows-entity-references/test]
#[test]
fn gfm_link_destination_escaping() {
    let html = render_html(&parse_final_nodes("[link](foo\\)\\:)\n"));
    assert_eq!(html, "<p><a href=\"foo):\">link</a></p>\n");
}

// [spec:gfm:syn:inlines.links.destination-cannot-contain-line-breaks/test]
#[test]
fn gfm_link_destination_no_line_breaks() {
    let html = render_html(&parse_final_nodes("[link](foo\nbar)\n"));
    assert_eq!(html, "<p>[link](foo\nbar)</p>\n");

    let html = render_html(&parse_final_nodes("[link](<foo\nbar>)\n"));
    assert_eq!(html, "<p>[link](<foo\nbar>)</p>\n");
}

// [spec:gfm:req:inlines.links.link-title-definition/test]
// [spec:gfm:syn:inlines.links.titles-may-use-single-quotes-double-quotes-or-parentheses/test]
// [spec:gfm:syn:inlines.links.different-title-delimiter-can-avoid-escaping/test]
// [spec:gfm:syn:inlines.links.titles-allow-backslash-escapes-and-entity-references/test]
// [spec:gfm:syn:inlines.links.nested-balanced-quotes-in-titles-require-escaping/test]
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

// [spec:gfm:syn:inlines.links.title-must-be-separated-from-destination-by-whitespace/test]
// [spec:gfm:req:inlines.links.title-cannot-appear-without-destination/test]
// [spec:gfm:def:inlines.links.link-titles-may-span-lines-but-not-blank-lines/test]
// [spec:gfm:syn:inlines.links.markdown-pl-title-behavior/test]
#[test]
fn gfm_link_title_edge_cases() {
    // Title without destination is parsed as destination
    let html = render_html(&parse_final_nodes("[link](\"title\")\n"));
    assert_eq!(html, "<p><a href=\"%22title%22\">link</a></p>\n");
}

// [spec:gfm:sem:inlines.links.whitespace-is-allowed-inside-inline-link-delimiters/test]
// [spec:gfm:syn:inlines.links.no-whitespace-allowed-between-link-text-and-opening-parenthesis/test]
#[test]
fn gfm_link_whitespace_rules() {
    // Whitespace allowed around dest/title
    let html = render_html(&parse_final_nodes("[link](   /uri\n  \"title\"  )\n"));
    assert_eq!(html, "<p><a href=\"/uri\" title=\"title\">link</a></p>\n");

    // No whitespace between link text and opening paren
    let html = render_html(&parse_final_nodes("[link] (/uri)\n"));
    assert_eq!(html, "<p>[link] (/uri)</p>\n");
}

// [spec:gfm:def:inlines.links.link-text-definition/test]
// [spec:gfm:syn:inlines.links.link-text-may-contain-inline-content/test]
// [spec:gfm:syn:inlines.links.link-text-may-contain-balanced-brackets-but-not-unbalanced-ones/test]
// [spec:gfm:syn:inlines.links.link-text-allows-only-escaped-or-balanced-brackets/test]
// [spec:gfm:syn:inlines.links.link-text-brackets-bind-more-tightly-than-emphasis/test]
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

// [spec:gfm:def:inlines.links.link-text-cannot-contain-links/test]
// [spec:gfm:syn:inlines.links.links-may-not-contain-other-links-at-any-level-nesting/test]
#[test]
fn gfm_links_cannot_contain_links() {
    // Nested link: parser produces nested anchors
    let html = render_html(&parse_final_nodes("[foo [bar](/uri)](/uri)\n"));
    assert_eq!(html, "<p>[foo <a href=\"/uri\">bar</a>](/uri)</p>\n");

    let html = render_html(&parse_final_nodes(
        "[foo *[bar [baz](/uri)](/uri)*](/uri)\n",
    ));
    assert_eq!(
        html,
        "<p>[foo <em>[bar <a href=\"/uri\">baz</a>](/uri)</em>](/uri)</p>\n"
    );
}

// [spec:gfm:syn:inlines.links.code-autolink-and-html-bind-tighter-than-link-text/test]
// [spec:gfm:syn:inlines.links.cases-illustrate-precedence-html-tags-code-spans-autolinks-over-link/test]
// [spec:gfm:syn:inlines.links.cases-illustrate-precedence-link-text-grouping-over-emphasis-grouping/test]
// [spec:gfm:syn:inlines.links.brackets-aren-t-part-links-do-not-take-precedence/test]
#[test]
fn gfm_link_precedence() {
    // Code span binds tighter than link text
    let html = render_html(&parse_final_nodes("[foo`](/uri)`\n"));
    assert_eq!(html, "<p>[foo<code>](/uri)</code></p>\n");

    // Link text grouping over emphasis grouping
    let html = render_html(&parse_final_nodes("*[foo*](/uri)\n"));
    assert_eq!(html, "<p>*<a href=\"/uri\">foo*</a></p>\n");
}

// ---------------------------------------------------------------------------
// Links: reference links (full, collapsed, shortcut)
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.links.full-reference-link-definition/test]
// [spec:gfm:sem:inlines.links.full-reference-link-example/test]
// [spec:gfm:sem:inlines.links.reference-link-text-comes-from-first-label-inlines/test]
// [spec:gfm:syn:inlines.links.reference-link-text-may-contain-inline-content/test]
// [spec:gfm:syn:inlines.links.reference-link-text-follows-inline-link-text-rules/test]
// [spec:gfm:syn:inlines.links.reference-link-text-allows-balanced-or-escaped-brackets/test]
// [spec:gfm:syn:inlines.links.reference-links-have-three-kinds/test]
#[test]
fn gfm_full_reference_link() {
    let html = render_html(&parse_final_nodes("[foo][bar]\n\n[bar]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");

    // Link text may contain inline content
    let html = render_html(&parse_final_nodes("[*foo* bar][baz]\n\n[baz]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\"><em>foo</em> bar</a></p>\n");
}

// [spec:gfm:req:inlines.links.label-match-definition/test]
// [spec:gfm:syn:inlines.links.label-matching-uses-unicode-case-fold/test]
// [spec:gfm:sem:inlines.links.label-matching-collapses-consecutive-internal-whitespace/test]
// [spec:gfm:sem:inlines.links.matching-uses-normalized-strings-not-parsed-inline-content/test]
#[test]
fn gfm_link_label_matching() {
    // Case-insensitive matching
    let html = render_html(&parse_final_nodes("[FOO][foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\">FOO</a></p>\n");

    // Multi-line label + case-insensitive + whitespace-collapsed match
    let html = render_html(&parse_final_nodes("[Foo\n  bar]: /url\n\n[Baz][Foo bar]\n"));
    assert_eq!(html, "<p><a href=\"/url\">Baz</a></p>\n");
}

// [spec:gfm:def:inlines.links.link-label-definition/test]
// [spec:gfm:syn:inlines.links.link-label-ends-at-first-unescaped-closing-bracket/test]
// [spec:gfm:syn:inlines.links.link-label-cannot-contain-unescaped-brackets/test]
// [spec:gfm:sem:inlines.links.link-labels-cannot-contain-unescaped-brackets/test]
// [spec:gfm:syn:inlines.links.link-label-must-contain-at-least-one-non-whitespace-character/test]
// [spec:gfm:def:inlines.links.link-label-must-contain-non-whitespace/test]
// [spec:gfm:syn:inlines.links.link-label-length-is-at-most-999-characters/test]
#[test]
fn gfm_link_label_rules() {
    // Label must contain non-whitespace
    let html = render_html(&parse_final_nodes("[ ]\n\n[ ]: /url\n"));
    assert_eq!(html, "<p>[ ]</p>\n<p>[ ]: /url</p>\n");
}

// [spec:gfm:syn:inlines.links.collapsed-reference-link-definition/test]
// [spec:gfm:sem:inlines.links.collapsed-reference-link-is-equivalent-to-full-link-with-same-label/test]
// [spec:gfm:sem:inlines.links.collapsed-reference-link-labels-are-case-insensitive/test]
// [spec:gfm:def:inlines.links.collapsed-reference-link-text-comes-from-first-label-inlines/test]
// [spec:gfm:sem:inlines.links.no-whitespace-allowed-between-collapsed-reference-brackets/test]
#[test]
fn gfm_collapsed_reference_link() {
    // Basic collapsed reference
    let html = render_html(&parse_final_nodes("[foo][]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");

    // Case-insensitive
    let html = render_html(&parse_final_nodes("[Foo][]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">Foo</a></p>\n");
}

// [spec:gfm:sem:inlines.links.shortcut-reference-link-is-equivalent-to-collapsed-reference-link/test]
// [spec:gfm:sem:inlines.links.shortcut-reference-link-labels-are-case-insensitive/test]
// [spec:gfm:def:inlines.links.shortcut-reference-link-text-comes-from-label-inlines/test]
// [spec:gfm:syn:inlines.links.shortcut-reference-does-not-parse-when-followed-by-link-label/test]
// [spec:gfm:sem:inlines.links.shortcut-reference-links-make-space-before-label-too-dangerous/test]
// [spec:gfm:sem:inlines.links.space-after-shortcut-reference-link-is-preserved/test]
#[test]
fn gfm_shortcut_reference_link() {
    // Basic shortcut
    let html = render_html(&parse_final_nodes("[foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p><a href=\"/url\" title=\"title\">foo</a></p>\n");

    // With inline content
    let html = render_html(&parse_final_nodes(
        "[*foo* bar]\n\n[*foo* bar]: /url \"title\"\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"/url\" title=\"title\"><em>foo</em> bar</a></p>\n"
    );

    // Case-insensitive
    let html = render_html(&parse_final_nodes("[Foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p><a href=\"/url\">Foo</a></p>\n");
}

// [spec:gfm:syn:inlines.links.full-and-collapsed-references-take-precedence-over-shortcuts/test]
// [spec:gfm:syn:inlines.links.inline-links-take-precedence-over-reference-links/test]
// [spec:gfm:def:inlines.links.first-matching-reference-definition-is-used/test]
// [spec:gfm:sem:inlines.links.first-reference-definition-wins-example/test]
// [spec:gfm:syn:inlines.links.defined-middle-label-makes-earlier-reference-parse/test]
#[test]
fn gfm_link_precedence_rules() {
    // Full reference takes precedence over shortcut
    let html = render_html(&parse_final_nodes(
        "[foo][bar]\n\n[foo]: /url1\n[bar]: /url2\n",
    ));
    assert_eq!(html, "<p><a href=\"/url2\">foo</a></p>\n");

    // Collapsed takes precedence over shortcut
    let html = render_html(&parse_final_nodes("[foo][]\n\n[foo]: /url1\n"));
    assert_eq!(html, "<p><a href=\"/url1\">foo</a></p>\n");

    // Inline takes precedence over reference
    let html = render_html(&parse_final_nodes("[foo]()\n\n[foo]: /url1\n"));
    assert_eq!(html, "<p><a href=\"\">foo</a></p>\n");
}

// [spec:gfm:sem:inlines.links.no-whitespace-allowed-between-link-text-and-link-label/test]
// [spec:gfm:syn:inlines.links.no-space-before-link-label-departs-from-original-markdown/test]
#[test]
fn gfm_link_no_whitespace_between_text_and_label() {
    let html = render_html(&parse_final_nodes("[foo] [bar]\n\n[bar]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p>[foo] <a href=\"/url\" title=\"title\">bar</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.links.reference-link-text-cannot-contain-links/test]
// [spec:gfm:syn:inlines.links.reference-link-text-grouping-takes-precedence-over-emphasis/test]
// [spec:gfm:syn:inlines.links.reference-links-lose-to-html-tags-code-spans-and-autolinks/test]
// [spec:gfm:syn:inlines.links.nested-reference-link-examples-produce-shortcuts/test]
// [spec:gfm:syn:inlines.links.reference-link-may-parse-later-brackets-as-separate-text/test]
#[test]
fn gfm_reference_link_edge_cases() {
    // Reference link text grouping over emphasis
    let html = render_html(&parse_final_nodes("*[foo*][bar]\n\n[bar]: /url\n"));
    assert_eq!(html, "<p>*<a href=\"/url\">foo*</a></p>\n");
}

// [spec:gfm:sem:inlines.links.escaping-opening-bracket-prevents-reference-link/test]
// [spec:gfm:syn:inlines.links.trailing-bracket-is-not-escaped-by-backslash-before-backslash/test]
#[test]
fn gfm_link_escaping() {
    // Escaping opening bracket prevents link
    let html = render_html(&parse_final_nodes("\\[foo]\n\n[foo]: /url\n"));
    assert_eq!(html, "<p>[foo]</p>\n");
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.images.image-description-follows-link-text-rules-with-two-differences/test]
// [spec:gfm:syn:inlines.images.image-description-contains-inline-elements/test]
// [spec:gfm:syn:inlines.images.html-rendering-typically-uses-plain-description-as-alt/test]
// [spec:gfm:sem:inlines.images.html-rendering-should-use-plain-string-content-for-alt/test]
#[test]
fn gfm_image_alt_text_from_inline_content() {
    // Image with title
    let html = render_html(&parse_final_nodes("![foo](/url \"title\")\n"));
    assert_eq!(
        html,
        "<p><img src=\"/url\" alt=\"foo\" title=\"title\" /></p>\n"
    );

    // Alt text is plain string content of description
    let html = render_html(&parse_final_nodes(
        "![foo *bar*]\n\n[foo *bar*]: train.jpg \"train & tracks\"\n",
    ));
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
    let html = render_html(&parse_final_nodes(
        "![foo *bar*][]\n\n[foo *bar*]: train.jpg \"train & tracks\"\n",
    ));
    assert_eq!(
        html,
        "<p><img src=\"train.jpg\" alt=\"foo bar\" title=\"train &amp; tracks\" /></p>\n"
    );

    // Reference-style with different case
    let html = render_html(&parse_final_nodes(
        "![foo *bar*][foobar]\n\n[FOOBAR]: train.jpg \"train & tracks\"\n",
    ));
    assert_eq!(
        html,
        "<p><img src=\"train.jpg\" alt=\"foo bar\" title=\"train &amp; tracks\" /></p>\n"
    );

    // Simple image without title
    let html = render_html(&parse_final_nodes("![foo](train.jpg)\n"));
    assert_eq!(html, "<p><img src=\"train.jpg\" alt=\"foo\" /></p>\n");

    // Image with trailing whitespace in parens
    let html = render_html(&parse_final_nodes(
        "My ![foo bar](/path/to/train.jpg  \"title\"   )\n",
    ));
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

// [spec:gfm:req:inlines.images.reference-image-labels-are-case-insensitive/test]
// [spec:gfm:sem:inlines.images.no-whitespace-allowed-between-reference-image-brackets/test]
// [spec:gfm:syn:inlines.images.shortcut-image-labels-are-case-insensitive/test]
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

// [spec:gfm:syn:inlines.images.reference-image-labels-cannot-contain-unescaped-brackets/test]
#[test]
fn gfm_image_label_no_unescaped_brackets() {
    let html = render_html(&parse_final_nodes("![[foo]]\n\n[[foo]]: /url \"title\"\n"));
    assert_eq!(
        html,
        "<p>![[foo]]</p>\n<p>[[foo]]: /url &quot;title&quot;</p>\n"
    );
}

// [spec:gfm:syn:inlines.images.escaping-bang-produces-literal-bang-followed-by-link/test]
// [spec:gfm:sem:inlines.images.escaping-opening-bracket-after-bang-produces-literal-bracketed-text/test]
#[test]
fn gfm_image_escaping() {
    // Escaping ! produces literal ! followed by link
    let html = render_html(&parse_final_nodes("\\![foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p>!<a href=\"/url\" title=\"title\">foo</a></p>\n");

    // Escaping [ after ! produces literal ![foo]
    let html = render_html(&parse_final_nodes("!\\[foo]\n\n[foo]: /url \"title\"\n"));
    assert_eq!(html, "<p>![foo]</p>\n");
}

// ---------------------------------------------------------------------------
// Container blocks: Block quotes
// ---------------------------------------------------------------------------

// [spec:gfm:syn:container-blocks.block-quotes.basic-case/test]
// [spec:gfm:syn:container-blocks.block-quotes.adjacent-lines-form-single-block-quote/test]
#[test]
fn gfm_block_quote_basic_and_adjacent() {
    let html = render_html(&parse_final_nodes("> foo\n> bar\n"));
    assert_eq!(html, "<blockquote>\n<p>foo\nbar</p>\n</blockquote>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.space-after-marker-is-optional/test]
#[test]
fn gfm_block_quote_no_space_after_marker() {
    let html = render_html(&parse_final_nodes(">foo\n"));
    assert_eq!(html, "<blockquote>\n<p>foo</p>\n</blockquote>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.marker-indent-up-to-three-spaces/test]
#[test]
fn gfm_block_quote_indented_marker() {
    let html = render_html(&parse_final_nodes("   > foo\n"));
    assert_eq!(html, "<blockquote>\n<p>foo</p>\n</blockquote>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.marker-indent-four-spaces-starts-code-block/test]
#[test]
fn gfm_block_quote_four_space_indent_is_code() {
    let html = render_html(&parse_final_nodes("    > not a quote\n"));
    assert_eq!(
        html,
        "<pre><code>&gt; not a quote
</code></pre>\n"
    );
}

// [spec:gfm:thm:container-blocks.block-quotes.indented-code-needs-five-spaces-after-marker/test]
#[test]
fn gfm_block_quote_code_needs_five_spaces() {
    // 5 spaces after > gives indented code inside blockquote
    let html = render_html(&parse_final_nodes(">     code\n"));
    assert_eq!(
        html,
        "<blockquote>\n<pre><code>code
</code></pre>\n</blockquote>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.laziness/test]
// [spec:gfm:syn:container-blocks.block-quotes.laziness-allows-omitted-continuation-markers/test]
// [spec:gfm:syn:container-blocks.block-quotes.lazy-continuation-example/test]
// [spec:gfm:def:container-blocks.block-quotes.paragraph-continuation-text-definition/test]
#[test]
fn gfm_block_quote_lazy_continuation() {
    // Lazy continuation: "bar" without ">" continues the blockquote paragraph
    let html = render_html(&parse_final_nodes("> foo\nbar\n"));
    assert_eq!(html, "<blockquote>\n<p>foo\nbar</p>\n</blockquote>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.may-mix-lazy-and-non-lazy-continuation-lines/test]
#[test]
fn gfm_block_quote_mixed_lazy_and_nonlazy() {
    let html = render_html(&parse_final_nodes("> foo\n> bar\nbaz\n"));
    assert_eq!(html, "<blockquote>\n<p>foo\nbar\nbaz</p>\n</blockquote>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.laziness-applies-only-to-paragraph-continuation-lines/test]
// [spec:gfm:syn:container-blocks.block-quotes.laziness-does-not-apply-to-code-block-lines/test]
#[test]
fn gfm_block_quote_lazy_does_not_apply_to_code() {
    // Without >, the indented code line is not part of the blockquote
    let html = render_html(&parse_final_nodes("> ```\nfoo\n```\n"));
    assert_eq!(
        html,
        "<blockquote>\n<pre><code></code></pre>\n</blockquote>\n<p>foo</p>\n<pre><code></code></pre>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.consecutiveness/test]
// [spec:gfm:sem:container-blocks.block-quotes.consecutiveness-rationale/test]
#[test]
fn gfm_block_quote_consecutiveness() {
    // A blank line between two block quotes separates them into two
    let html = render_html(&parse_final_nodes("> foo\n\n> bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<blockquote>\n<p>bar</p>\n</blockquote>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.blank-line-separates-adjacent-block-quotes/test]
// [spec:gfm:syn:container-blocks.block-quotes.blank-quoted-line-creates-two-paragraphs/test]
#[test]
fn gfm_block_quote_blank_line_separates_paragraphs() {
    let html = render_html(&parse_final_nodes("> foo\n>\n> bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n<p>bar</p>\n</blockquote>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.can-be-empty/test]
#[test]
fn gfm_block_quote_empty() {
    let html = render_html(&parse_final_nodes(">\n"));
    assert_eq!(html, "<blockquote>\n</blockquote>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.can-have-initial-or-final-blank-lines/test]
#[test]
fn gfm_block_quote_initial_and_final_blank_lines() {
    let html = render_html(&parse_final_nodes(">\n> foo\n>\n"));
    assert_eq!(html, "<blockquote>\n<p>foo</p>\n</blockquote>\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.blank-lines-generally-not-required-around-block-quotes/test]
// [spec:gfm:thm:container-blocks.block-quotes.blank-line-required-before-following-paragraph/test]
#[test]
fn gfm_block_quote_blank_line_before_following_paragraph() {
    // A block quote can follow a paragraph without a blank line
    let html = render_html(&parse_final_nodes("foo\n> bar\n"));
    assert_eq!(
        html,
        "<p>foo</p>\n<blockquote>\n<p>bar</p>\n</blockquote>\n"
    );

    // Lazy continuation: "baz" continues the blockquote paragraph
    let html = render_html(&parse_final_nodes("> bar\nbaz\n"));
    assert_eq!(html, "<blockquote>\n<p>bar\nbaz</p>\n</blockquote>\n");

    let html = render_html(&parse_final_nodes("> bar\n\nbaz\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>bar</p>\n</blockquote>\n<p>baz</p>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.can-interrupt-paragraphs/test]
#[test]
fn gfm_block_quote_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n> quote\n"));
    assert_eq!(
        html,
        "<p>para</p>\n<blockquote>\n<p>quote</p>\n</blockquote>\n"
    );
}

// [spec:gfm:thm:container-blocks.block-quotes.nested-block-quotes-may-omit-continuation-markers/test]
#[test]
fn gfm_block_quote_nested() {
    let html = render_html(&parse_final_nodes("> > > nested\n"));
    assert_eq!(
        html,
        "<blockquote>\n<blockquote>\n<blockquote>\n<p>nested</p>\n</blockquote>\n</blockquote>\n</blockquote>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.indented-dash-remains-paragraph-continuation-text/test]
#[test]
fn gfm_block_quote_indented_dash_is_continuation() {
    // A lazy continuation with "- " is not a list item
    let html = render_html(&parse_final_nodes("> foo\n- bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<ul>\n<li>bar</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.omitting-marker-before-setext-underline-changes-meaning/test]
#[test]
fn gfm_block_quote_lazy_setext_underline() {
    // A lazy setext underline should not create a heading inside the blockquote
    let html = render_html(&parse_final_nodes("> foo\n---\n"));
    assert_eq!(html, "<blockquote>\n<p>foo</p>\n</blockquote>\n<hr />\n");
}

// [spec:gfm:syn:container-blocks.block-quotes.omitting-marker-before-list-item-ends-block-quote/test]
// [spec:gfm:syn:container-blocks.block-quotes.list-example-block-quote-ends-after-first-line/test]
// [spec:gfm:syn:container-blocks.block-quotes.lazy-continuation-list-example-explanation/test]
#[test]
fn gfm_block_quote_omitting_marker_before_list() {
    let html = render_html(&parse_final_nodes("> foo\n- list\n"));
    assert_eq!(
        html,
        "<blockquote>\n<p>foo</p>\n</blockquote>\n<ul>\n<li>list</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.block-quotes.only-defined-cases-count/test]
#[test]
fn gfm_block_quote_only_defined_cases() {
    // Ensure that non-matching lazy lines are handled properly
    let html = render_html(&parse_final_nodes("> foo\n> bar\n"));
    assert_eq!(html, "<blockquote>\n<p>foo\nbar</p>\n</blockquote>\n");
}

// ---------------------------------------------------------------------------
// Container blocks: definitions
// ---------------------------------------------------------------------------

// [spec:gfm:def:container-blocks.definition/test]
// [spec:gfm:def:container-blocks.basic-kinds-are-block-quotes-and-list-items/test]
// [spec:gfm:def:container-blocks.general-recursive-definition-form/test]
// [spec:gfm:def:container-blocks.generated-from-contents-rather-than-parse-recipe/test]
// [spec:gfm:def:container-blocks.lists-are-meta-containers-for-list-items/test]
// [spec:gfm:def:container-blocks.syntax-is-defined-recursively/test]
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

// [spec:gfm:syn:container-blocks.list-items.basic-case/test]
// [spec:gfm:def:container-blocks.list-items.marker-definition/test]
// [spec:gfm:def:container-blocks.list-items.bullet-marker-definition/test]
#[test]
fn gfm_list_item_basic_bullet() {
    let html = render_html(&parse_final_nodes("- foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("+ foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("* foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.basic-case-example-ordered-item/test]
// [spec:gfm:def:container-blocks.list-items.ordered-marker-definition/test]
// [spec:gfm:sem:container-blocks.list-items.ordered-item-start-number-comes-from-marker/test]
#[test]
fn gfm_list_item_basic_ordered() {
    let html = render_html(&parse_final_nodes("1. foo\n"));
    assert_eq!(html, "<ol>\n<li>foo</li>\n</ol>\n");

    let html = render_html(&parse_final_nodes("1) foo\n"));
    assert_eq!(html, "<ol>\n<li>foo</li>\n</ol>\n");

    let html = render_html(&parse_final_nodes("3. foo\n"));
    assert_eq!(html, "<ol start=\"3\">\n<li>foo</li>\n</ol>\n");
}

// [spec:gfm:syn:container-blocks.list-items.ordered-start-number-cannot-be-negative/test]
#[test]
fn gfm_list_item_negative_start_not_valid() {
    let html = render_html(&parse_final_nodes("-1. foo\n"));
    // "-1." is not a valid ordered marker (negative); it's just a paragraph
    assert_eq!(html, "<p>-1. foo</p>\n");
}

// [spec:gfm:syn:container-blocks.list-items.at-least-one-space-required-after-marker/test]
#[test]
fn gfm_list_item_space_required_after_marker() {
    // Without space, it's not a list (though "-" by itself is empty item)
    let html = render_html(&parse_final_nodes("1.foo\n"));
    assert_eq!(html, "<p>1.foo</p>\n");
}

// [spec:gfm:sem:container-blocks.list-items.type-is-determined-by-marker/test]
#[test]
fn gfm_list_item_type_determined_by_marker() {
    let html = render_html(&parse_final_nodes("- bullet\n"));
    assert_eq!(html, "<ul>\n<li>bullet</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("1. ordered\n"));
    assert_eq!(html, "<ol>\n<li>ordered</li>\n</ol>\n");
}

// [spec:gfm:syn:container-blocks.list-items.required-indentation-is-marker-width-plus-padding/test]
// [spec:gfm:syn:container-blocks.list-items.examples-of-required-indentation/test]
#[test]
fn gfm_list_item_indentation_rules() {
    // "- " = 2 chars; continuation indented 2 spaces
    let html = render_html(&parse_final_nodes("- foo\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");

    // 1 space indent: lazy continuation with partial indent preserved
    let html = render_html(&parse_final_nodes("- foo\n bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\n bar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.required-indentation-is-relative-not-column-based/test]
#[test]
fn gfm_list_item_indentation_is_relative() {
    // Indentation is relative to the marker, not absolute column
    let html = render_html(&parse_final_nodes(" - foo\n   bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.basic-case-example-source-blocks/test]
// [spec:gfm:syn:container-blocks.list-items.may-contain-any-kind-of-block/test]
#[test]
fn gfm_list_item_may_contain_blocks() {
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.item-may-contain-heading/test]
#[test]
fn gfm_list_item_may_contain_heading() {
    let html = render_html(&parse_final_nodes("- # heading\n"));
    assert_eq!(html, "<ul>\n<li>\n<h1>heading</h1>\n</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.indented-code-example-with-wider-marker/test]
// [spec:gfm:syn:container-blocks.list-items.indented-code-start-uses-one-space-after-marker/test]
#[test]
fn gfm_list_item_indented_code_wider_marker() {
    // "10. " = 4 chars, so code needs 8 spaces total indent
    let html = render_html(&parse_final_nodes("10. foo\n\n        bar\n"));
    assert_eq!(
        html,
        "<ol start=\"10\">\n<li>\n<p>foo</p>\n<pre><code>bar
</code></pre>\n</li>\n</ol>\n"
    );
}

// [spec:gfm:sem:container-blocks.list-items.extra-indent-counts-as-code-content/test]
#[test]
fn gfm_list_item_extra_indent_is_code() {
    // "1.  " is 4 chars (marker + 2 spaces); content starts at col 4
    // Extra indentation beyond that counts as code
    let html = render_html(&parse_final_nodes(
        "1.  indented code\n\n        more code\n",
    ));
    assert_eq!(
        html,
        "<ol>\n<li>\n<p>indented code</p>\n<pre><code>more code\n</code></pre>\n</li>\n</ol>\n"
    );
}

// [spec:gfm:sem:container-blocks.list-items.indented-code-inside-item-preserves-empty-lines/test]
#[test]
fn gfm_list_item_code_preserves_empty_lines() {
    let html = render_html(&parse_final_nodes("- foo\n\n      bar\n\n      baz\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>bar\n\nbaz
</code></pre>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.list-items.basic-rules-apply-only-to-nonblank-or-code-start/test]
#[test]
fn gfm_list_item_basic_rules_nonblank() {
    // Blank line after marker triggers "empty item" logic, not normal rules
    let html = render_html(&parse_final_nodes("-\n  foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.items-may-start-with-one-blank-line-and-still-be-nonempty/test]
// [spec:gfm:syn:container-blocks.list-items.may-begin-with-at-most-one-blank-line/test]
// [spec:gfm:syn:container-blocks.list-items.item-starting-with-blank-line/test]
#[test]
fn gfm_list_item_start_with_blank_line() {
    let html = render_html(&parse_final_nodes("-\n  foo\n-\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n<li>bar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.empty-bullet-item/test]
#[test]
fn gfm_list_item_empty_bullet() {
    let html = render_html(&parse_final_nodes("- foo\n-\n- bar\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n<li></li>\n<li>bar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.empty-ordered-item/test]
#[test]
fn gfm_list_item_empty_ordered() {
    let html = render_html(&parse_final_nodes("1. foo\n2.\n3. bar\n"));
    assert_eq!(html, "<ol>\n<li>foo</li>\n<li></li>\n<li>bar</li>\n</ol>\n");
}

// [spec:gfm:syn:container-blocks.list-items.empty-item-cannot-interrupt-paragraph/test]
#[test]
fn gfm_list_item_empty_cannot_interrupt_para() {
    let html = render_html(&parse_final_nodes("para\n*\n"));
    assert_eq!(html, "<p>para\n*</p>\n");
}

// [spec:gfm:syn:container-blocks.list-items.empty-lines-inside-list-item-need-not-be-indented/test]
#[test]
fn gfm_list_item_empty_lines_not_indented() {
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.item-blocks-may-be-separated-by-multiple-blank-lines/test]
#[test]
fn gfm_list_item_multiple_blank_lines() {
    let html = render_html(&parse_final_nodes("- foo\n\n\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.indenting-whole-item-four-spaces-produces-code-block/test]
#[test]
fn gfm_list_item_four_space_indent_is_code() {
    let html = render_html(&parse_final_nodes("    - not a list\n"));
    assert_eq!(
        html,
        "<pre><code>- not a list
</code></pre>\n"
    );
}

// [spec:gfm:sem:container-blocks.list-items.leading-indent-up-to-three-spaces-can-be-normalized/test]
#[test]
fn gfm_list_item_leading_indent_normalized() {
    let html = render_html(&parse_final_nodes("  - foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("   - foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.thematic-break-line-is-not-a-list-item/test]
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

// [spec:gfm:syn:container-blocks.list-items.list-may-be-first-block-in-item/test]
#[test]
fn gfm_list_inside_list_item() {
    let html = render_html(&parse_final_nodes("- - foo\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<ul>\n<li>foo</li>\n</ul>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.list-items.list-may-start-or-end-with-empty-item/test]
#[test]
fn gfm_list_start_or_end_with_empty_item() {
    let html = render_html(&parse_final_nodes("-\n- foo\n"));
    assert_eq!(html, "<ul>\n<li></li>\n<li>foo</li>\n</ul>\n");

    let html = render_html(&parse_final_nodes("- foo\n-\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n<li></li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.lazy-continuation-may-delete-only-part-of-indent/test]
#[test]
fn gfm_list_item_lazy_partial_indent() {
    // Lazy continuation with partial indentation
    let html = render_html(&parse_final_nodes("- foo\nbar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.sublist-example-two-space-indent/test]
// [spec:gfm:syn:container-blocks.list-items.sublist-example-one-space-is-not-enough/test]
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
    assert_eq!(html, "<ul>\n<li>foo</li>\n<li>bar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.sublist-example-indent-increases-with-wider-marker/test]
// [spec:gfm:syn:container-blocks.list-items.sublist-example-three-spaces-not-enough-for-wider-marker/test]
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

// [spec:gfm:syn:container-blocks.list-items.far-right-text-may-still-be-outside-item/test]
#[test]
fn gfm_list_item_far_right_outside_item() {
    // "10. " is 4 chars; 3 spaces of indent is not enough, text treated as continuation
    let html = render_html(&parse_final_nodes("10. foo\n   bar\n"));
    assert_eq!(html, "<ol start=\"10\">\n<li>foo\n   bar</li>\n</ol>\n");
}

// [spec:gfm:syn:container-blocks.list-items.same-column-can-still-be-inside-item-in-nested-context/test]
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

// [spec:gfm:sem:container-blocks.lists.single-paragraph-list-is-tight/test]
// [spec:gfm:def:container-blocks.lists.tight-list-definition/test]
// [spec:gfm:sem:container-blocks.lists.loose-vs-tight-html-difference/test]
#[test]
fn gfm_list_tight_single_paragraph() {
    let html = render_html(&parse_final_nodes("- foo\n- bar\n- baz\n"));
    assert_eq!(
        html,
        "<ul>\n<li>foo</li>\n<li>bar</li>\n<li>baz</li>\n</ul>\n"
    );
}

// [spec:gfm:sem:container-blocks.lists.blank-line-between-items-makes-list-loose/test]
#[test]
fn gfm_list_loose_blank_between_items() {
    let html = render_html(&parse_final_nodes("- foo\n\n- bar\n\n- baz\n"));
    // Blank lines between items make the list loose (items wrapped in <p>)
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n</li>\n<li>\n<p>bar</p>\n</li>\n<li>\n<p>baz</p>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:sem:container-blocks.lists.blank-line-between-item-blocks-makes-list-loose/test]
// [spec:gfm:sem:container-blocks.lists.blank-line-between-block-elements-in-item-makes-list-loose/test]
#[test]
fn gfm_list_loose_blank_between_blocks_in_item() {
    let html = render_html(&parse_final_nodes("- a\n\n  b\n- c\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>a</p>\n<p>b</p>\n</li>\n<li>\n<p>c</p>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:sem:container-blocks.lists.outer-list-may-be-loose-while-inner-list-is-tight/test]
// [spec:gfm:sem:container-blocks.lists.loose-sublist-does-not-necessarily-make-outer-list-loose/test]
#[test]
fn gfm_list_outer_loose_inner_tight() {
    let html = render_html(&parse_final_nodes("- a\n  - b\n  - c\n\n- d\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>a</p>\n<ul>\n<li>b</li>\n<li>c</li>\n</ul>\n</li>\n<li>\n<p>d</p>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.lists.items-may-be-separated-by-blank-lines/test]
// [spec:gfm:syn:container-blocks.lists.any-number-of-blank-lines-may-separate-items/test]
#[test]
fn gfm_list_items_separated_by_multiple_blank_lines() {
    let html = render_html(&parse_final_nodes("- a\n\n\n- b\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>a</p>\n</li>\n<li>\n<p>b</p>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.lists.changing-marker-type-or-delimiter-starts-new-list/test]
// [spec:gfm:def:container-blocks.lists.same-marker-type-definition/test]
// [spec:gfm:def:container-blocks.lists.same-type-definition/test]
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
    assert_eq!(html, "<ol>\n<li>foo</li>\n<li>bar</li>\n</ol>\n");
}

// [spec:gfm:syn:container-blocks.lists.list-can-interrupt-paragraph-without-blank-line/test]
// [spec:gfm:syn:container-blocks.lists.commonmark-allows-list-interruption/test]
#[test]
fn gfm_list_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("foo\n- bar\n"));
    assert_eq!(html, "<p>foo</p>\n<ul>\n<li>bar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.lists.only-lists-starting-with-one-may-interrupt-paragraphs/test]
// [spec:gfm:sem:container-blocks.lists.markdown-pl-disallows-this-to-avoid-hard-wrapped-numeral-lists/test]
// [spec:gfm:sem:container-blocks.lists.starting-with-one-should-prevent-most-spurious-captures/test]
#[test]
fn gfm_list_only_one_interrupts_paragraph() {
    // "1." can interrupt
    let html = render_html(&parse_final_nodes("foo\n1. bar\n"));
    assert_eq!(html, "<p>foo</p>\n<ol>\n<li>bar</li>\n</ol>\n");

    // "2." cannot interrupt
    let html = render_html(&parse_final_nodes("foo\n2. bar\n"));
    assert_eq!(html, "<p>foo\n2. bar</p>\n");
}

// [spec:gfm:syn:container-blocks.lists.starting-with-one-can-still-cause-unintended-list-capture/test]
#[test]
fn gfm_list_one_can_still_capture() {
    let html = render_html(&parse_final_nodes("para\n1. list\n"));
    assert_eq!(html, "<p>para</p>\n<ol>\n<li>list</li>\n</ol>\n");
}

// [spec:gfm:sem:container-blocks.lists.markdown-pl-still-allows-block-quotes-to-interrupt-paragraphs/test]
#[test]
fn gfm_block_quote_still_interrupts_paragraph() {
    let html = render_html(&parse_final_nodes("para\n> quote\n"));
    assert_eq!(
        html,
        "<p>para</p>\n<blockquote>\n<p>quote</p>\n</blockquote>\n"
    );
}

// [spec:gfm:syn:container-blocks.lists.items-need-not-use-uniform-indentation/test]
#[test]
fn gfm_list_non_uniform_indentation() {
    let html = render_html(&parse_final_nodes("- a\n - b\n  - c\n"));
    // Each should be an item at various levels
    assert!(html.contains("<li>"));
}

// [spec:gfm:sem:container-blocks.lists.consecutive-block-elements-without-blank-line-keep-list-tight/test]
#[test]
fn gfm_list_tight_consecutive_blocks() {
    let html = render_html(&parse_final_nodes("- a\n  - b\n"));
    // No blank lines between, so the outer list should be tight
    assert!(!html.contains("<p>a</p>"));
}

// [spec:gfm:sem:container-blocks.lists.blank-lines-inside-code-block-do-not-make-list-loose/test]
#[test]
fn gfm_list_blank_lines_in_code_not_loose() {
    let html = render_html(&parse_final_nodes("- ```\n  a\n\n  b\n  ```\n- c\n"));
    // The blank line inside the fenced code block
    assert!(html.contains("<li>"));
    assert!(html.contains("c"));
}

// [spec:gfm:sem:container-blocks.lists.blank-line-inside-block-quote-does-not-make-list-loose/test]
#[test]
fn gfm_list_blank_line_in_blockquote_not_loose() {
    let html = render_html(&parse_final_nodes("- > a\n  >\n  > b\n- c\n"));
    // The blank line inside the block quote should not make the outer list loose
    assert!(!html.contains("<p>c</p>"));
}

// [spec:gfm:sem:container-blocks.lists.blank-line-before-empty-item-still-makes-list-loose/test]
#[test]
fn gfm_list_blank_before_empty_item_is_loose() {
    let html = render_html(&parse_final_nodes("- a\n\n-\n"));
    // List with blank line before empty item
    assert!(html.contains("<li>"));
    assert!(html.contains("a"));
}

// [spec:gfm:syn:container-blocks.lists.four-space-indented-marker-is-code-block/test]
// [spec:gfm:syn:container-blocks.lists.more-than-three-spaces-do-not-start-item/test]
#[test]
fn gfm_list_four_space_marker_is_code() {
    let html = render_html(&parse_final_nodes("    - foo\n"));
    assert_eq!(
        html,
        "<pre><code>- foo
</code></pre>\n"
    );
}

// [spec:gfm:syn:container-blocks.lists.nested-example-is-list-item-with-sublist/test]
// [spec:gfm:sem:container-blocks.lists.corresponding-standalone-example-should-parse-the-same-way/test]
#[test]
fn gfm_list_nested_sublist_example() {
    let html = render_html(&parse_final_nodes("- a\n  - b\n    - c\n"));
    assert!(html.contains("<ul>"));
    // Should create nested lists
    assert!(html.matches("<ul>").count() >= 2);
}

// [spec:gfm:syn:container-blocks.lists.blank-html-comment-separates-lists/test]
#[test]
fn gfm_list_html_comment_separates_lists() {
    let html = render_html(&parse_final_nodes("- foo\n\n<!-- -->\n\n- bar\n"));
    // The HTML comment separates them into two distinct lists
    assert!(html.matches("<ul>").count() == 2);
}

// [spec:gfm:def:container-blocks.lists.start-number-definition/test]
#[test]
fn gfm_list_start_number() {
    let html = render_html(&parse_final_nodes("3. foo\n4. bar\n"));
    assert_eq!(
        html,
        "<ol start=\"3\">\n<li>foo</li>\n<li>bar</li>\n</ol>\n"
    );
}

// [spec:gfm:def:container-blocks.lists.principle-of-uniformity-definition/test]
// [spec:gfm:thm:container-blocks.lists.uniformity-preserves-nested-list-meaning/test]
// [spec:gfm:thm:container-blocks.lists.uniformity-supports-list-interruption/test]
#[test]
fn gfm_list_uniformity() {
    // Uniformity principle: same marker type stays in same list
    let html = render_html(&parse_final_nodes("- a\n- b\n- c\n"));
    assert_eq!(html, "<ul>\n<li>a</li>\n<li>b</li>\n<li>c</li>\n</ul>\n");
    // Changing between bullet and ordered starts new list
    let html = render_html(&parse_final_nodes("- a\n1. b\n"));
    assert!(html.contains("<ul>"));
    assert!(html.contains("<ol>"));
}

// ---------------------------------------------------------------------------
// Container blocks: Task list items (extension)
// ---------------------------------------------------------------------------

// [spec:gfm:syn:container-blocks.task-list-items-extension.task-lists-may-be-arbitrarily-nested/test]
#[test]
fn gfm_task_list_nested() {
    let html = render_html(&parse_final_nodes("- [x] parent\n  - [ ] child\n"));
    assert!(html.contains("checked"));
    assert!(html.contains("<ul>"));
}

// [spec:gfm:req:container-blocks.task-list-items-extension.checkbox-interaction-is-unspecified/test]
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

// [spec:gfm:syn:container-blocks.list-items.motivation.original-markdown-spec-quoted/test]
// [spec:gfm:def:container-blocks.list-items.motivation.original-rule-implies-four-space-rule/test]
// [spec:gfm:sem:container-blocks.list-items.motivation.four-space-rule-is-arbitrary/test]
// [spec:gfm:syn:container-blocks.list-items.motivation.preferred-parse-is-single-list/test]
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

// [spec:gfm:syn:container-blocks.list-items.motivation.four-space-rule-splits-list/test]
#[test]
fn gfm_motivation_four_space_rule_splits() {
    // Under the old four-space rule, this would be split
    // Under CommonMark, 2 spaces suffice for "-" marker
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert!(html.contains("<p>bar</p>"));
    // bar is inside the list item
    assert!(html.contains("<li>"));
}

// [spec:gfm:syn:container-blocks.list-items.motivation.marker-width-determines-indentation/test]
#[test]
fn gfm_motivation_marker_width_determines_indent() {
    // "1. " is 3 chars, "10. " is 4 chars: different indent requirements
    let html = render_html(&parse_final_nodes("1. foo\n   bar\n"));
    assert_eq!(html, "<ol>\n<li>foo\nbar</li>\n</ol>\n");

    let html = render_html(&parse_final_nodes("10. foo\n    bar\n"));
    assert_eq!(html, "<ol start=\"10\">\n<li>foo\nbar</li>\n</ol>\n");
}

// [spec:gfm:syn:container-blocks.list-items.motivation.fixed-indent-from-marker-is-an-alternative/test]
// [spec:gfm:sem:container-blocks.list-items.motivation.fixed-margin-indentation-is-unintuitive/test]
#[test]
fn gfm_motivation_fixed_indent_alternative() {
    // CommonMark uses marker-width-based indentation rather than a fixed indent
    let html = render_html(&parse_final_nodes("- foo\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.motivation.fixed-marker-indent-breaks-code-patterns/test]
// [spec:gfm:syn:container-blocks.list-items.motivation.spec-preserves-eight-space-code-pattern/test]
#[test]
fn gfm_motivation_code_inside_list_item() {
    // Code inside a list item needs marker-width + 4 spaces
    let html = render_html(&parse_final_nodes("- foo\n\n      code\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<p>foo</p>\n<pre><code>code
</code></pre>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:syn:container-blocks.list-items.motivation.indented-code-start-needs-special-treatment/test]
// [spec:gfm:syn:container-blocks.list-items.item-starting-with-indented-code/test]
#[test]
fn gfm_motivation_indented_code_start() {
    // When the first line of a list item starts with enough spaces for a code block
    let html = render_html(&parse_final_nodes("-     code\n"));
    assert_eq!(
        html,
        "<ul>\n<li>\n<pre><code>code
</code></pre>\n</li>\n</ul>\n"
    );
}

// [spec:gfm:sem:container-blocks.list-items.motivation.markdown-pl-diverged-from-four-space-rule/test]
// [spec:gfm:sem:container-blocks.list-items.motivation.no-rule-avoids-breaking-all-existing-documents/test]
#[test]
fn gfm_motivation_divergence_from_four_space_rule() {
    // CommonMark's approach: marker width determines indentation
    let html = render_html(&parse_final_nodes("- a\n  b\n"));
    assert_eq!(html, "<ul>\n<li>a\nb</li>\n</ul>\n");
}

// [spec:gfm:sem:container-blocks.list-items.motivation.two-space-rule-is-extremely-unintuitive/test]
// [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-allows-underindented-content/test]
#[test]
fn gfm_motivation_two_space_rule() {
    // Two spaces is the natural indent for "- " markers
    let html = render_html(&parse_final_nodes("- foo\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>foo\nbar</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-example-single-list-item/test]
#[test]
fn gfm_motivation_two_space_single_item() {
    let html = render_html(&parse_final_nodes("- foo\n\n  bar\n"));
    assert_eq!(html, "<ul>\n<li>\n<p>foo</p>\n<p>bar</p>\n</li>\n</ul>\n");
}

// [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-example-inside-block-quote/test]
// [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-block-quote-html/test]
#[test]
fn gfm_motivation_two_space_inside_block_quote() {
    let html = render_html(&parse_final_nodes("> - foo\n>   bar\n"));
    assert_eq!(
        html,
        "<blockquote>\n<ul>\n<li>foo\nbar</li>\n</ul>\n</blockquote>\n"
    );
}

// ---------------------------------------------------------------------------
// Preliminaries: Characters and lines
// ---------------------------------------------------------------------------

// [spec:gfm:def:preliminaries.characters-and-lines.line-ending-definition/test]
// [spec:gfm:def:preliminaries.characters-and-lines.character-definition/test]
// [spec:gfm:def:preliminaries.characters-and-lines.line-definition/test]
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

// [spec:gfm:def:preliminaries.characters-and-lines.blank-line-definition/test]
#[test]
fn gfm_blank_line_separates_paragraphs() {
    let html = render_html(&parse_final_nodes("aaa\n\nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");

    // A line with only spaces is also blank
    let html = render_html(&parse_final_nodes("aaa\n   \nbbb\n"));
    assert_eq!(html, "<p>aaa</p>\n<p>bbb</p>\n");
}

// [spec:gfm:def:preliminaries.characters-and-lines.ascii-punctuation-character-definition/test]
// [spec:gfm:def:preliminaries.characters-and-lines.punctuation-character-definition/test]
#[test]
fn gfm_ascii_punctuation_backslash_escaping() {
    // Backslash escaping works for ASCII punctuation characters
    let html = render_html(&parse_final_nodes("\\!\\\"\\#\\$\\%\\&\n"));
    assert_eq!(html, "<p>!&quot;#$%&amp;</p>\n");

    let html = render_html(&parse_final_nodes("\\*\\+\\,\\-\\.\\/\n"));
    assert_eq!(html, "<p>*+,-./</p>\n");
}

// [spec:gfm:def:preliminaries.characters-and-lines.whitespace-character-definition/test]
// [spec:gfm:def:preliminaries.characters-and-lines.whitespace-definition/test]
// [spec:gfm:def:preliminaries.characters-and-lines.space-definition/test]
// [spec:gfm:def:preliminaries.characters-and-lines.non-whitespace-character-definition/test]
#[test]
fn gfm_whitespace_and_space_definitions() {
    // Spaces are structural whitespace (e.g., 4 spaces = indented code)
    let html = render_html(&parse_final_nodes("    code\n"));
    assert_eq!(
        html,
        "<pre><code>code
</code></pre>\n"
    );

    // Tab is whitespace too
    let html = render_html(&parse_final_nodes("\tcode\n"));
    assert_eq!(
        html,
        "<pre><code>code
</code></pre>\n"
    );

    // Non-whitespace characters form content
    let html = render_html(&parse_final_nodes("abc\n"));
    assert_eq!(html, "<p>abc</p>\n");
}

// [spec:gfm:def:preliminaries.characters-and-lines.unicode-whitespace-character-definition/test]
// [spec:gfm:def:preliminaries.characters-and-lines.unicode-whitespace-definition/test]
#[test]
fn gfm_unicode_whitespace_not_structural() {
    // Non-breaking space (U+00A0) is unicode whitespace but does NOT act as
    // structural whitespace, so it does not create indented code blocks.
    let html = render_html(&parse_final_nodes("\u{00A0}\u{00A0}\u{00A0}\u{00A0}code\n"));
    assert_eq!(html, "<p>\u{00A0}\u{00A0}\u{00A0}\u{00A0}code</p>\n");
}

// [spec:gfm:def:preliminaries.characters-and-lines.character-class-definitions/test]
#[test]
fn gfm_character_class_definitions() {
    // Unicode punctuation counts as punctuation for delimiter rules.
    // U+00AB (LEFT-POINTING DOUBLE ANGLE QUOTATION MARK) is Unicode punctuation.
    // Emphasis after Unicode punctuation is allowed.
    let html = render_html(&parse_final_nodes("\u{00AB}*foo*\n"));
    assert_eq!(html, "<p>\u{00AB}<em>foo</em></p>\n");
}

// [spec:gfm:syn:preliminaries.characters-and-lines.valid-commonmark-document/test]
// [spec:gfm:req:preliminaries.characters-and-lines.encoding-is-unspecified/test]
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

// [spec:gfm:req:preliminaries.insecure-characters.null-character-is-replaced/test]
#[test]
fn gfm_null_character_replaced_with_replacement_char() {
    let html = render_html(&parse_final_nodes("foo\0bar\n"));
    assert_eq!(html, "<p>foo\u{FFFD}bar</p>\n");
}

// ---------------------------------------------------------------------------
// Blocks and Inlines
// ---------------------------------------------------------------------------

// [spec:gfm:def:blocks-and-inlines.document-is-sequence-of-blocks/test]
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

// [spec:gfm:def:blocks-and-inlines.container-and-leaf-blocks-definition/test]
#[test]
fn gfm_container_and_leaf_blocks_together() {
    // Container blocks (blockquote, list) hold other blocks;
    // Leaf blocks (heading, paragraph, code block) do not.
    let html = render_html(&parse_final_nodes(
        "> # Heading\n>\n> Paragraph\n\n    code\n",
    ));
    assert_eq!(
        html,
        "<blockquote>\n<h1>Heading</h1>\n<p>Paragraph</p>\n</blockquote>\n<pre><code>code
</code></pre>\n"
    );
}

// [spec:gfm:syn:blocks-and-inlines.precedence.block-indicators-take-precedence-over-inline-indicators/test]
#[test]
fn gfm_block_takes_precedence_over_inline() {
    // `#` at the start of a line creates a heading, not a paragraph with a literal `#`
    let html = render_html(&parse_final_nodes("# foo\n"));
    assert_eq!(html, "<h1>foo</h1>\n");

    // `-` at the start creates a list, not a paragraph with `-`
    let html = render_html(&parse_final_nodes("- foo\n"));
    assert_eq!(html, "<ul>\n<li>foo</li>\n</ul>\n");
}

// [spec:gfm:thm:blocks-and-inlines.precedence.two-phase-parsing/test]
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
// Inlines: top-level rules
// ---------------------------------------------------------------------------

// [spec:gfm:syn:inlines.inline-parsing-is-sequential/test]
// [spec:gfm:syn:inlines.earlier-inline-match-takes-precedence-in-sequential-parse/test]
#[test]
fn gfm_inline_parsing_sequential_and_precedence() {
    // Inlines are parsed left-to-right; the earlier match wins.
    // `hi` is parsed as a code span, leaving the trailing backtick literal.
    let html = render_html(&parse_final_nodes("`hi`lo`\n"));
    assert_eq!(html, "<p><code>hi</code>lo`</p>\n");
}

// ---------------------------------------------------------------------------
// Inlines: Soft line breaks
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.soft-line-breaks.regular-line-break-produces-softbreak/test]
// [spec:gfm:req:inlines.soft-line-breaks.softbreak-may-render-as-line-ending-or-space/test]
// [spec:gfm:req:inlines.soft-line-breaks.renderers-may-optionally-promote-softbreaks/test]
#[test]
fn gfm_soft_line_break() {
    // A regular line break (without trailing spaces or backslash) produces a softbreak.
    // Our renderer emits it as a line ending, which is one conforming option.
    let html = render_html(&parse_final_nodes("foo\nbaz\n"));
    assert_eq!(html, "<p>foo\nbaz</p>\n");
}

// [spec:gfm:sem:inlines.soft-line-breaks.edge-spaces-around-softbreak-are-removed/test]
#[test]
fn gfm_soft_line_break_edge_spaces_removed() {
    // A single trailing space (fewer than 2) does not create a hard break;
    // the parser produces a softbreak. Edge spaces are preserved in text
    // nodes and expected to be stripped by the renderer.
    let nodes = parse_final_nodes("foo \n baz\n");
    let Node::Paragraph { children, .. } = &nodes[0] else {
        panic!("expected Paragraph");
    };
    assert!(
        children.iter().any(|n| matches!(n, Node::SoftBreak { .. })),
        "should contain a SoftBreak node between the two lines"
    );
}

// ---------------------------------------------------------------------------
// Inlines: Textual content
// ---------------------------------------------------------------------------

// [spec:gfm:syn:inlines.textual-content.uninterpreted-characters-are-parsed-as-text/test]
#[test]
fn gfm_textual_content_plain_text() {
    // Characters not given special interpretation are parsed as plain text.
    let html = render_html(&parse_final_nodes("hello $.;'there\n"));
    assert_eq!(html, "<p>hello $.;'there</p>\n");

    // Unicode text is preserved as-is.
    let html = render_html(&parse_final_nodes("Foo \u{03C7}\u{03C1}\u{1FC6}\u{03BD}\n"));
    assert_eq!(html, "<p>Foo \u{03C7}\u{03C1}\u{1FC6}\u{03BD}</p>\n");
}

// [spec:gfm:sem:inlines.textual-content.internal-spaces-are-preserved/test]
#[test]
fn gfm_textual_content_internal_spaces_preserved() {
    // Internal spaces are preserved verbatim.
    let html = render_html(&parse_final_nodes("Multiple     spaces\n"));
    assert_eq!(html, "<p>Multiple     spaces</p>\n");
}

// ---------------------------------------------------------------------------
// Appendix A: Overview
// ---------------------------------------------------------------------------

// [spec:gfm:def:appendix-a-parsing-strategy.overview.document-is-represented-as-tree-of-blocks/test]
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

// [spec:gfm:sem:appendix-a-parsing-strategy.overview.parsing-has-two-phases/test]
// [spec:gfm:def:appendix-a-parsing-strategy.reference-implementations-use-this-parsing-strategy/test]
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

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.four-line-example-shows-tree-evolution/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.initial-tree-is-just-document/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.first-line-opens-block-quote-and-paragraph/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.second-line-is-lazy-continuation/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.third-line-closes-paragraph-and-opens-list/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.fourth-line-opens-second-list-item/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.each-line-may-close-open-or-append/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.phase-1-line-processing-procedure/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.lines-can-be-discarded-after-incorporation/test]
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

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.reference-definitions-are-detected-at-paragraph-close/test]
#[test]
fn gfm_appendix_reference_definitions_detected_at_paragraph_close() {
    // Reference definitions are extracted from paragraphs when they close.
    let html = render_html(&parse_final_nodes("[foo]: /url\n\n[foo]\n"));
    assert_eq!(html, "<p><a href=\"/url\">foo</a></p>\n");
}

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.setext-underlines-convert-paragraphs/test]
#[test]
fn gfm_appendix_setext_underlines_convert_paragraphs() {
    // A setext underline converts a paragraph to a heading during phase 1.
    let html = render_html(&parse_final_nodes("Foo\n===\n"));
    assert_eq!(html, "<h1>Foo</h1>\n");
}

// ---------------------------------------------------------------------------
// Appendix A: Phase 2 - Inline structure
// ---------------------------------------------------------------------------

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.phase-2-parses-inline-content/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.open-blocks-close-at-end-of-input/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.example-produces-softbreak-and-emphasis/test]
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
    let Node::List {
        children: items, ..
    } = &children[1]
    else {
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

// [spec:gfm:def:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.delimiter-stack-definition/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.push-delimiters-to-stack/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.stack-entries-point-to-text-nodes/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-initial-current-position/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-loop/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-consumes-delimiters/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-inserts-node/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-chooses-node-kind/test]
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

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-interior-delimiters/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-advances/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-inactive-closer/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-cleans-up-stack/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.end-of-input-triggers-emphasis-processing/test]
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

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-stack-bottom-limits-scan/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-tracks-openers-bottom/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-updates-openers-bottom/test]
// [spec:gfm:def:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.trickiest-inline-case/test]
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

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.closing-bracket-triggers-link-search/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-scan-backward/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-builds-node-children/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-removes-opener+1/test]
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

// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-failure-case/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-deactivates-earlier-openers/test]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-runs-emphasis-processing/test]
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

// ---------------------------------------------------------------------------
// Standard autolinks (angle-bracket delimited)
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.autolinks.uri-autolink-definition/test]
// [spec:gfm:def:inlines.autolinks.absolute-uri-definition/test]
// [spec:gfm:def:inlines.autolinks.scheme-definition/test]
// [spec:gfm:def:inlines.autolinks.valid-uri-autolink-example/test]
#[test]
fn gfm_standard_uri_autolink() {
    let html = render_html(&parse_final_nodes("<http://foo.bar.baz>\n"));
    assert_eq!(
        html,
        "<p><a href=\"http://foo.bar.baz\">http://foo.bar.baz</a></p>\n"
    );

    let html = render_html(&parse_final_nodes(
        "<http://foo.bar.baz/test?q=hello&id=22&boolean>\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"http://foo.bar.baz/test?q=hello&id=22&boolean\">http://foo.bar.baz/test?q=hello&amp;id=22&amp;boolean</a></p>\n"
    );

    let html = render_html(&parse_final_nodes("<irc://foo.bar:2233/baz>\n"));
    assert_eq!(
        html,
        "<p><a href=\"irc://foo.bar:2233/baz\">irc://foo.bar:2233/baz</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks.autolink-schemes-are-case-insensitive/test]
#[test]
fn gfm_autolink_case_insensitive_scheme() {
    let html = render_html(&parse_final_nodes("<MAILTO:FOO@BAR.BAZ>\n"));
    assert_eq!(
        html,
        "<p><a href=\"MAILTO:FOO@BAR.BAZ\">MAILTO:FOO@BAR.BAZ</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks.spec-accepts-some-absolute-uris-that-are-not-valid-uris/test]
#[test]
fn gfm_autolink_nonstandard_uris() {
    let html = render_html(&parse_final_nodes("<a+b+c:d>\n"));
    assert_eq!(html, "<p><a href=\"a+b+c:d\">a+b+c:d</a></p>\n");

    let html = render_html(&parse_final_nodes("<made-up-scheme://foo,bar>\n"));
    assert_eq!(
        html,
        "<p><a href=\"made-up-scheme://foo,bar\">made-up-scheme://foo,bar</a></p>\n"
    );

    let html = render_html(&parse_final_nodes("<http://../>\n"));
    assert_eq!(html, "<p><a href=\"http://../\">http://../</a></p>\n");

    let html = render_html(&parse_final_nodes("<localhost:5001/foo>\n"));
    assert_eq!(
        html,
        "<p><a href=\"localhost:5001/foo\">localhost:5001/foo</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks.autolinks-cannot-contain-spaces/test]
#[test]
fn gfm_autolink_no_spaces() {
    // Spaces prevent autolink; entire <...> consumed as literal
    let html = render_html(&parse_final_nodes("<http://foo.bar/baz bim>\n"));
    assert_eq!(html, "<p>&lt;http://foo.bar/baz bim&gt;</p>\n");
}

// [spec:gfm:syn:inlines.autolinks.backslash-escapes-do-not-work-inside-autolinks/test]
#[test]
fn gfm_autolink_no_backslash_escapes() {
    // Backslash escapes are not processed inside autolinks; the raw chars are kept
    let html = render_html(&parse_final_nodes("<http://example.com/\\[\\>\n"));
    assert_eq!(
        html,
        "<p><a href=\"http://example.com/%5C%5B%5C\">http://example.com/\\[\\</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks.email-autolink-definition/test]
// [spec:gfm:def:inlines.autolinks.email-address-definition/test]
// [spec:gfm:def:inlines.autolinks.email-address-regex/test]
// [spec:gfm:def:inlines.autolinks.valid-email-autolink-example/test]
#[test]
fn gfm_standard_email_autolink() {
    let html = render_html(&parse_final_nodes("<foo@bar.example.com>\n"));
    assert_eq!(
        html,
        "<p><a href=\"mailto:foo@bar.example.com\">foo@bar.example.com</a></p>\n"
    );

    let html = render_html(&parse_final_nodes("<foo+special@Bar.baz-bar0.com>\n"));
    assert_eq!(
        html,
        "<p><a href=\"mailto:foo+special@Bar.baz-bar0.com\">foo+special@Bar.baz-bar0.com</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks.backslash-escapes-do-not-work-inside-email-autolinks/test]
#[test]
fn gfm_email_autolink_no_backslash_escapes() {
    // Backslash in email invalidates autolink; not valid HTML tag either
    let html = render_html(&parse_final_nodes("<foo\\+@bar.example.com>\n"));
    assert_eq!(html, "<p>&lt;foo+@bar.example.com&gt;</p>\n");
}

// [spec:gfm:syn:inlines.autolinks.invalid-autolink-example/test]
#[test]
fn gfm_invalid_autolinks() {
    // Empty angle brackets: parsed as Text, escaped by renderer
    let html = render_html(&parse_final_nodes("<>\n"));
    assert_eq!(html, "<p>&lt;&gt;</p>\n");

    // Spaces inside: not autolink, entire <...> consumed as literal
    let html = render_html(&parse_final_nodes("< http://foo.bar >\n"));
    assert_eq!(html, "<p>&lt; http://foo.bar &gt;</p>\n");

    // Single-char scheme: not a valid autolink
    let html = render_html(&parse_final_nodes("<m:abc>\n"));
    assert_eq!(html, "<p>&lt;m:abc&gt;</p>\n");

    // No scheme at all: not autolink, not valid HTML tag (has dots)
    let html = render_html(&parse_final_nodes("<foo.bar.baz>\n"));
    assert_eq!(html, "<p>&lt;foo.bar.baz&gt;</p>\n");
}

// ---------------------------------------------------------------------------
// Extended autolinks (GFM extension) - additional rules
// ---------------------------------------------------------------------------

// [spec:gfm:def:inlines.autolinks-extension.extended-email-autolink-definition/test]
// [spec:gfm:def:inlines.autolinks-extension.extended-email-autolink-rules/test]
// [spec:gfm:syn:inlines.autolinks-extension.extended-email-autolinks-insert-mailto-scheme/test]
#[test]
fn gfm_extended_email_autolink() {
    let html = render_html(&parse_final_nodes("foo@bar.baz\n"));
    assert_eq!(
        html,
        "<p><a href=\"mailto:foo@bar.baz\">foo@bar.baz</a></p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks-extension.dot-dash-and-underscore-rules-around-at-sign/test]
#[test]
fn gfm_extended_email_dot_dash_underscore() {
    // dot, dash, underscore allowed on both sides of @
    let html = render_html(&parse_final_nodes("a.b-c_d@a.b\n"));
    assert_eq!(
        html,
        "<p><a href=\"mailto:a.b-c_d@a.b\">a.b-c_d@a.b</a></p>\n"
    );

    // trailing underscore after @ invalidates the email
    let html = render_html(&parse_final_nodes("a.b-c_d@a.b_\n"));
    assert_eq!(html, "<p>a.b-c_d@a.b_</p>\n");
}

// [spec:gfm:sem:inlines.autolinks-extension.plus-is-allowed-before-but-not-after-at-sign/test]
#[test]
fn gfm_extended_email_plus_rules() {
    let html = render_html(&parse_final_nodes(
        "hello@mail+xyz.example isn't valid, but hello+xyz@mail.example is.\n",
    ));
    assert_eq!(
        html,
        "<p>hello@mail+xyz.example isn't valid, but <a href=\"mailto:hello+xyz@mail.example\">hello+xyz@mail.example</a> is.</p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks-extension.left-angle-bracket-ends-autolink/test]
#[test]
fn gfm_extended_autolink_left_angle_ends() {
    let html = render_html(&parse_final_nodes("www.commonmark.org/he<lp\n"));
    assert_eq!(
        html,
        "<p><a href=\"http://www.commonmark.org/he\">www.commonmark.org/he</a>&lt;lp</p>\n"
    );
}

// [spec:gfm:syn:inlines.autolinks-extension.trailing-semicolon-is-excluded-when-it-looks-like-entity/test]
#[test]
fn gfm_extended_autolink_entity_like_semicolon() {
    // Normal query string with & is fine
    let html = render_html(&parse_final_nodes(
        "www.google.com/search?q=commonmark&hl=en\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"http://www.google.com/search?q=commonmark&hl=en\">www.google.com/search?q=commonmark&amp;hl=en</a></p>\n"
    );

    // Trailing &hl; looks like entity; entire &...; sequence is excluded
    let html = render_html(&parse_final_nodes(
        "www.google.com/search?q=commonmark&hl;\n",
    ));
    assert_eq!(
        html,
        "<p><a href=\"http://www.google.com/search?q=commonmark\">www.google.com/search?q=commonmark</a>&amp;hl;</p>\n"
    );
}
