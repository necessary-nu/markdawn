//! Surface, prop-inertness and totality contracts of the Dioxus adapter.

use super::*;

// ----- surface ----------------------------------------------------------

// [spec:mw:def:dioxus.surface.entry-points/test]
#[test]
fn the_configurable_layer_is_a_with_chain_over_renderer() {
    struct Rewriter;
    impl crate::ext::CodeBlockRewriter for Rewriter {
        fn rewrite(
            &self,
            info: Option<&str>,
            literal: &str,
        ) -> Option<crate::ext::CodeBlockRewrite> {
            (info == Some("shout")).then(|| crate::ext::CodeBlockRewrite {
                info: None,
                literal: literal.to_uppercase(),
            })
        }
    }

    // Each `with_*` returns `Self`, so the whole configuration is one chain
    // that produces a reusable renderer.
    let renderer: Renderer<()> = Renderer::default()
        .with_options(DioxusOptions::all())
        .with_html_filter(crate::HtmlFilter::unrestricted())
        .with_alert_icons(|_kind| rsx! { span { class: "icon", "*" } })
        .with_code_block_rewriter(Rewriter);

    // Every configured slot is observable in the output of `render`.
    let source = "> [!NOTE]\n> body\n\n<script>alert(1)</script>\n\n```shout\nquiet\n```\n";
    let html = dioxus_ssr::render_element(renderer.render(source));
    // with_options(all) enabled alerts; with_alert_icons supplied the glyph.
    assert!(html.contains("class=\"icon\""), "{html}");
    // with_html_filter(unrestricted) let the raw script through, where the
    // default safe filter drops it.
    assert!(html.contains("alert(1)"), "{html}");
    let safe = dioxus_ssr::render_element(
        Renderer::default()
            .with_options(DioxusOptions::all())
            .render(source),
    );
    assert!(!safe.contains("alert(1)"), "{safe}");
    // with_code_block_rewriter substituted the literal.
    assert!(html.contains("QUIET"), "{html}");

    // The same renderer is reusable across calls.
    assert_eq!(
        dioxus_ssr::render_element(renderer.render("a")),
        dioxus_ssr::render_element(renderer.render("a"))
    );
}

// [spec:mw:def:dioxus.surface.entry-points/test]
#[test]
fn render_takes_source_and_render_node_takes_a_parsed_node() {
    let renderer = Renderer::default();

    // `render` parses and renders a whole document.
    assert_eq!(
        dioxus_ssr::render_element(renderer.render("# h\n\np\n")),
        "<h1>h</h1><p>p</p>"
    );

    // `render_node` renders one already-parsed node, block or inline.
    let nodes = super::tests::parse_nodes("# h\n", DioxusOptions::default());
    assert_eq!(
        dioxus_ssr::render_element(renderer.render_node(&nodes[0])),
        "<h1>h</h1>"
    );
    let inline: Node<()> = Node::Text {
        text: "bare".to_owned(),
        source: crate::node::SourceSpan::default(),
    };
    assert_eq!(
        dioxus_ssr::render_element(renderer.render_node(&inline)),
        "bare"
    );
}

// [spec:mw:def:dioxus.surface.entry-points/test]
#[test]
fn the_convenience_layer_delegates_to_the_configurable_one() {
    let source = "# h\n\n[x](https://e.com)\n";
    let options = DioxusOptions::all();

    // render_markdown == Renderer::default().with_options(options).render
    assert_eq!(
        dioxus_ssr::render_element(render_markdown(source, options)),
        dioxus_ssr::render_element(Renderer::default().with_options(options).render(source))
    );

    // The `Markdown` component delegates to `render_markdown`.
    assert_eq!(
        dioxus_ssr::render_element(rsx! { Markdown { source: source.to_string(), options } }),
        dioxus_ssr::render_element(render_markdown(source, options))
    );

    // The free `render_node` builds a throwaway renderer borrowing `custom`,
    // so it renders with default options and the safe HTML filter.
    let raw: Node<()> = Node::Html {
        html: "<b>keep</b><script>drop()</script>".to_owned(),
        source: crate::node::SourceSpan::default(),
    };
    let free = dioxus_ssr::render_element(render_node(&raw, &()));
    assert!(free.contains("keep"), "{free}");
    assert!(
        !free.contains("<script>"),
        "safe filter not applied: {free}"
    );
    assert_eq!(
        free,
        dioxus_ssr::render_element(Renderer::default().render_node(&raw))
    );
}

// [spec:mw:def:dioxus.surface.entry-points/test]
#[test]
fn the_custom_hook_is_a_no_op_for_the_unit_type() {
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Payload(&'static str);

    struct Renders;
    impl RenderDioxusCustom<Payload> for Renders {
        fn render_custom(&self, payload: &Payload, _children: &[Node<Payload>]) -> Element {
            let text = payload.0;
            rsx! { span { "{text}" } }
        }
    }

    let node: Node<Payload> = Node::Custom {
        payload: Payload("x^2"),
        children: Vec::new(),
        source: crate::node::SourceSpan::default(),
    };

    // `()` renders nothing; a real implementation renders its payload.
    assert_eq!(dioxus_ssr::render_element(render_node(&node, &())), "");
    assert_eq!(
        dioxus_ssr::render_element(render_node(&node, &Renders)),
        "<span>x^2</span>"
    );
}

// [spec:mw:def:dioxus.surface.entry-points/test]
#[test]
fn render_borrows_self_while_the_highlighter_slot_is_interior_mutable() {
    // `render` / `render_node` take `&self`, so a renderer behind a shared
    // reference — which is all a Dioxus component ever has — can render.
    fn render_behind_shared_ref(renderer: &Renderer<()>) -> String {
        dioxus_ssr::render_element(renderer.render("plain\n"))
    }
    let renderer = Renderer::default();
    assert_eq!(render_behind_shared_ref(&renderer), "<p>plain</p>");

    // The `&mut self` highlighter is reached through a RefCell, so it can
    // still be driven from a `&self` render.
    #[cfg(feature = "highlight")]
    {
        struct Counting(std::rc::Rc<core::cell::Cell<usize>>);
        impl crate::highlight::SyntaxHighlighter for Counting {
            fn highlight(
                &mut self,
                _language_hint: &str,
                _source: &str,
            ) -> Result<Vec<crate::highlight::SyntaxSpan>, crate::highlight::HighlightError>
            {
                self.0.set(self.0.get() + 1);
                Ok(Vec::new())
            }
        }
        let calls = std::rc::Rc::new(core::cell::Cell::new(0));
        let renderer = Renderer::default().with_highlighter(Counting(calls.clone()));
        let _ = dioxus_ssr::render_element(renderer.render("```rust\nfn main() {}\n```\n"));
        assert_eq!(calls.get(), 1, "&mut self highlighter was not reached");
    }
}

// ----- prop inertness ---------------------------------------------------

// [spec:mw:req:dioxus.props.inert/test]
#[test]
fn options_are_plain_copyable_values_satisfying_the_prop_bounds() {
    fn assert_prop_bounds<T: Clone + PartialEq + Default>() {}
    assert_prop_bounds::<DioxusOptions>();
    assert_prop_bounds::<String>();

    // `Copy` — passing it by value leaves the original usable.
    let options = DioxusOptions::all();
    let copied = options;
    assert_eq!(copied, options);

    // `Eq` and a `Default` that enables nothing.
    let default = DioxusOptions::default();
    assert_ne!(default, options);
    assert_eq!(default, DioxusOptions::default());
    assert!(!default.gfm);
    assert!(!default.math);
    assert!(!default.alerts);
    assert!(!default.highlights);
    assert!(!default.definition_lists);
}

// [spec:mw:req:dioxus.props.inert/test]
#[test]
fn the_component_props_are_owned_values_that_compare_by_equality() {
    fn assert_prop_bounds<T: Clone + PartialEq>() {}
    assert_prop_bounds::<MarkdownProps>();

    let props = MarkdownProps {
        source: "same".to_owned(),
        options: DioxusOptions::default(),
    };
    // Prop-equality memoization depends on this comparison being structural
    // — no closure, trait object or interior mutability in the way.
    // (`MarkdownProps` is not `Debug`, so compare with `==`.)
    assert!(props.clone() == props);
    assert!(
        MarkdownProps {
            source: "other".to_owned(),
            options: DioxusOptions::default(),
        } != props
    );
    assert!(
        MarkdownProps {
            source: "same".to_owned(),
            options: DioxusOptions::all(),
        } != props
    );
}

// [spec:mw:req:dioxus.props.inert/test]
#[test]
fn the_component_renders_identically_under_ssr_and_in_a_live_virtual_dom() {
    let source = "# h\n\n- a\n- b\n\n[x](https://e.com)\n";
    let options = DioxusOptions::all();

    let direct =
        dioxus_ssr::render_element(rsx! { Markdown { source: source.to_string(), options } });

    let owned = source.to_string();
    let mut vdom = VirtualDom::new_with_props(
        Markdown,
        MarkdownProps {
            source: owned,
            options,
        },
    );
    vdom.rebuild_in_place();
    let live = dioxus_ssr::render(&vdom);

    assert_eq!(live, direct);
    assert_eq!(
        live,
        dioxus_ssr::render_element(render_markdown(source, options))
    );

    // No hooks or signals means rebuilding the same props again is stable.
    vdom.rebuild_in_place();
    assert_eq!(dioxus_ssr::render(&vdom), live);
}

// [spec:mw:req:dioxus.props.inert/test]
#[test]
fn the_alert_icon_is_invoked_once_per_alert_and_placed_inline() {
    let calls = std::rc::Rc::new(core::cell::Cell::new(0usize));
    let seen = calls.clone();
    let renderer = Renderer::default()
        .with_options(DioxusOptions {
            alerts: true,
            ..DioxusOptions::default()
        })
        .with_alert_icons(move |kind| {
            seen.set(seen.get() + 1);
            let name = format!("{kind:?}");
            rsx! { i { class: "ico", "{name}" } }
        });

    let html =
        dioxus_ssr::render_element(renderer.render("> [!NOTE]\n> one\n\n> [!WARNING]\n> two\n"));
    assert_eq!(calls.get(), 2, "icon fn ran {} times: {html}", calls.get());
    // The callback's result is inline in the title row, not stored as a
    // callback in the tree.
    assert!(html.contains("<i class=\"ico\">Note</i>"), "{html}");
    assert!(html.contains("<i class=\"ico\">Warning</i>"), "{html}");
    assert!(!html.contains("onclick"), "{html}");
}

// [spec:mw:req:dioxus.props.inert/test]
#[test]
fn the_emitted_tree_carries_no_event_handlers() {
    // Every interactive-looking construct markdawn emits — links, task
    // checkboxes, footnote backrefs, the ansi pane toggle — must be inert
    // markup, so the same tree serialises identically server-side.
    let html = dioxus_ssr::render_element(render_markdown(
        "[link](https://e.com)\n\n\
         - [x] done\n\n\
         note[^1]\n\n[^1]: body\n",
        DioxusOptions::all(),
    ));
    for handler in [
        "onclick",
        "onchange",
        "oninput",
        "onmousedown",
        "onkeydown",
        "onsubmit",
    ] {
        assert!(!html.contains(handler), "{handler} in output: {html}");
    }
}

// ----- rewriters --------------------------------------------------------

// [spec:mw:req:core.rewriter.render-time-only/test]
#[test]
fn a_rewrite_affects_rendering_only_and_reruns_on_every_render() {
    struct Rewriter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl crate::ext::CodeBlockRewriter for Rewriter {
        fn rewrite(
            &self,
            info: Option<&str>,
            _literal: &str,
        ) -> Option<crate::ext::CodeBlockRewrite> {
            (info == Some("wire")).then(|| {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                crate::ext::CodeBlockRewrite {
                    info: Some("rendered".into()),
                    literal: "substituted".into(),
                }
            })
        }
    }

    let source = "```wire\noriginal body\n```\n";
    let nodes = super::tests::parse_nodes(source, DioxusOptions::default());
    let Node::CodeBlock { info, literal, .. } = &nodes[0] else {
        panic!("expected a code block, got {nodes:?}");
    };
    let (info_before, literal_before) = (info.clone(), literal.clone());

    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let renderer = Renderer::default().with_code_block_rewriter(Rewriter(calls.clone()));
    let seen = || calls.load(std::sync::atomic::Ordering::Relaxed);

    let first = dioxus_ssr::render_element(renderer.render_node(&nodes[0]));
    assert!(first.contains("substituted"), "{first}");
    assert!(first.contains("language-rendered"), "{first}");
    assert!(!first.contains("original body"), "{first}");
    assert_eq!(seen(), 1);

    // The parsed node kept its own `info` and `literal`.
    let Node::CodeBlock { info, literal, .. } = &nodes[0] else {
        unreachable!()
    };
    assert_eq!(info, &info_before);
    assert_eq!(literal, &literal_before);
    assert_eq!(info.as_deref(), Some("wire"));
    assert_eq!(literal, "original body\n");

    // So serializing the tree reproduces the source as written.
    assert_eq!(crate::serializer::serialize(&nodes[0]), source);

    // And re-rendering the same tree re-runs the rewriter.
    assert_eq!(
        dioxus_ssr::render_element(renderer.render_node(&nodes[0])),
        first
    );
    assert_eq!(seen(), 2);
}

// ----- totality ---------------------------------------------------------

// [spec:mw:sem:dioxus.render.totality/test]
#[test]
fn every_entry_point_returns_an_element_with_no_error_channel() {
    // Typed bindings, so a signature change to `Result<Element, _>` breaks
    // the build rather than silently gaining an error path.
    let renderer = Renderer::default();
    let _: Element = renderer.render("x");
    let _: Element = renderer.render_node(&Node::<()>::Text {
        text: "x".to_owned(),
        source: crate::node::SourceSpan::default(),
    });
    let _: Element = render_markdown("x", DioxusOptions::default());
    let _: Element = render_node(
        &Node::<()>::Text {
            text: "x".to_owned(),
            source: crate::node::SourceSpan::default(),
        },
        &(),
    );
    let _: Element = rsx! { Markdown { source: "x".to_string() } };
}

// [spec:mw:sem:dioxus.render.totality/test]
#[test]
fn a_custom_node_degrades_to_empty_where_the_html_adapter_errors() {
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Payload;

    let node: Node<Payload> = Node::Custom {
        payload: Payload,
        children: Vec::new(),
        source: crate::node::SourceSpan::default(),
    };

    // Dioxus: empty element, no diagnostic.
    assert_eq!(
        dioxus_ssr::render_element(Renderer::default().render_node(&node)),
        ""
    );

    // The equivalent HTML path surfaces an error for the same node.
    let mut out = String::new();
    let error = crate::adapter::html::RenderHtmlCustom::render_custom(&(), &Payload, &[], &mut out)
        .expect_err("() must reject a custom payload");
    assert_eq!(
        error,
        crate::adapter::html::HtmlError::UnsupportedCustomNode
    );
}

// [spec:mw:sem:dioxus.render.totality/test]
#[cfg(feature = "graphviz")]
#[test]
fn a_failing_diagram_backend_falls_through_to_plain_pre_code() {
    let options = DioxusOptions {
        graphviz: true,
        ..DioxusOptions::all()
    };
    // Not valid DOT, so the engine errors.
    let source = "```dot\nthis is not dot at all {{{\n```\n";
    let html = dioxus_ssr::render_element(render_markdown(source, options));

    assert!(
        !html.contains("<svg"),
        "engine error still emitted SVG: {html}"
    );
    assert!(html.contains("<pre>"), "{html}");
    assert!(html.contains("<code"), "{html}");
    assert!(html.contains("not dot at all"), "source was lost: {html}");
}

// [spec:mw:sem:dioxus.render.totality/test]
#[test]
fn the_adapter_exposes_no_streaming_surface() {
    // `render` consumes a whole source string; there is no `Sink` or
    // `StreamItem` counterpart to `HtmlSink`, so a provisional emission has
    // nowhere to go. The observable consequence: an unterminated construct
    // still renders as a complete document rather than being withheld.
    let html = dioxus_ssr::render_element(render_markdown(
        "```rust\nfn main() {",
        DioxusOptions::all(),
    ));
    assert!(html.contains("<pre>"), "{html}");
    assert!(html.contains("fn main() {"), "{html}");
}
