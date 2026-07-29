//! HTML adapter — a thin serializer over the canonical vdom renderer
//! (`adapter::vdom`). `HtmlAdapter` builds a `VNode` tree and walks it
//! to an HTML string; all node→structure decisions live in `vdom`.

use core::fmt;

use crate::adapter::escape::{escape_href, escape_html};
use crate::adapter::vdom::{AttrVal, Props, RenderVdomCustom, VNode, VdomAdapter, VdomError};
use crate::node::{Emission, Node, StreamItem};
use crate::sink::Sink;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlError {
    UnsupportedCustomNode,
    Sink(String),
}

impl fmt::Display for HtmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCustomNode => f.write_str("custom node renderer not configured"),
            Self::Sink(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for HtmlError {}

impl From<VdomError> for HtmlError {
    fn from(error: VdomError) -> Self {
        match error {
            VdomError::UnsupportedCustomNode => HtmlError::UnsupportedCustomNode,
            VdomError::Sink(message) => HtmlError::Sink(message),
        }
    }
}

impl From<HtmlError> for VdomError {
    fn from(error: HtmlError) -> Self {
        match error {
            HtmlError::UnsupportedCustomNode => VdomError::UnsupportedCustomNode,
            HtmlError::Sink(message) => VdomError::Sink(message),
        }
    }
}

// [spec:mw:def:html.custom.hook]
pub trait RenderHtmlCustom<Ext> {
    fn render_custom(
        &self,
        payload: &Ext,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), HtmlError>;
}

impl<Ext> RenderHtmlCustom<Ext> for () {
    fn render_custom(
        &self,
        _payload: &Ext,
        _children: &[Node<Ext>],
        _out: &mut String,
    ) -> Result<(), HtmlError> {
        Err(HtmlError::UnsupportedCustomNode)
    }
}

/// Adapts a string-based `RenderHtmlCustom` into the vdom renderer's
/// `RenderVdomCustom`: the custom HTML string becomes a raw node, with
/// the same trailing-newline rule the old `HtmlAdapter` applied.
struct HtmlCustomBridge<R>(R);

// [spec:mw:sem:html.custom.bridge]
impl<Ext, R> RenderVdomCustom<Ext> for HtmlCustomBridge<R>
where
    R: RenderHtmlCustom<Ext>,
{
    fn render_custom(&self, payload: &Ext, children: &[Node<Ext>]) -> Result<VNode, VdomError> {
        let mut html = String::new();
        self.0.render_custom(payload, children, &mut html)?;
        let nl_close = crate::adapter::vdom::is_block_like(children) && !html.ends_with('\n');
        Ok(VNode::Raw {
            tag: None,
            props: Props::new(),
            html,
            nl_close,
        })
    }
}

pub struct HtmlAdapter<R = ()> {
    inner: VdomAdapter<HtmlCustomBridge<R>>,
}

impl Default for HtmlAdapter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlAdapter<()> {
    pub fn new() -> Self {
        Self {
            inner: VdomAdapter::with_custom_renderer(HtmlCustomBridge(())),
        }
    }

    pub fn with_custom_renderer<Custom>(custom: Custom) -> HtmlAdapter<Custom> {
        HtmlAdapter {
            inner: VdomAdapter::with_custom_renderer(HtmlCustomBridge(custom)),
        }
    }
}

impl<R> HtmlAdapter<R> {
    /// Override the HTML tag filter applied to every `Node::Html`.
    /// Default is [`HtmlFilter::safe`]. Pass [`HtmlFilter::unrestricted`]
    /// to opt in to raw HTML.
    pub fn with_html_filter(mut self, filter: crate::HtmlFilter) -> Self {
        self.inner = self.inner.with_html_filter(filter);
        self
    }

    /// Reject `data:image/<mime>;…` URIs in `<img src>` (the default
    /// allows them — browsers sandbox scripts inside image-typed `data:`
    /// URIs loaded via `<img>`, so it's safe in that single context).
    pub fn without_data_image_urls(mut self) -> Self {
        self.inner = self.inner.without_data_image_urls();
        self
    }
}

#[cfg(feature = "highlight")]
impl<R> HtmlAdapter<R> {
    /// Install a renderer-neutral syntax highlighter. Failures fall back to
    /// plain `<pre><code>`.
    pub fn with_highlighter<H>(mut self, highlighter: H) -> Self
    where
        H: crate::highlight::SyntaxHighlighter + 'static,
    {
        self.inner = self.inner.with_highlighter(highlighter);
        self
    }
}

#[cfg(feature = "pikchr")]
impl<R> HtmlAdapter<R> {
    /// Render fenced code blocks tagged `pikchr` as inline SVG diagrams.
    pub fn with_pikchr(mut self) -> Self {
        self.inner = self.inner.with_pikchr();
        self
    }
}

#[cfg(feature = "typst")]
impl<R> HtmlAdapter<R> {
    /// Compile fenced code blocks tagged `typst` to inline SVG figures.
    pub fn with_typst(mut self) -> Self {
        self.inner = self.inner.with_typst();
        self
    }
}

#[cfg(feature = "graphviz")]
impl<R> HtmlAdapter<R> {
    /// Render fenced code blocks tagged `dot`/`graphviz` as inline SVG.
    pub fn with_graphviz(mut self) -> Self {
        self.inner = self.inner.with_graphviz();
        self
    }
}

impl<R> HtmlAdapter<R> {
    // [spec:mw:req:html.render.serializer-over-vdom]
    pub fn render<Ext>(&self, item: StreamItem<Node<Ext>>) -> Result<StreamItem<String>, HtmlError>
    where
        R: RenderHtmlCustom<Ext>,
    {
        match self.inner.render(item)? {
            StreamItem::Upsert(emission) => {
                let mut out = String::new();
                vnode_to_html(&emission.node, &mut out);
                Ok(StreamItem::Upsert(Emission {
                    id: emission.id,
                    provisional: emission.provisional,
                    node: out,
                }))
            }
            StreamItem::Delete { id } => Ok(StreamItem::Delete { id }),
        }
    }
}

// ----- VNode → HTML serializer -------------------------------------

// [spec:mw:sem:html.serialize.emission]
fn vnode_to_html(node: &VNode, out: &mut String) {
    match node {
        VNode::Text { text } => escape_html(text, out),
        VNode::Fragment { children } => {
            for child in children {
                vnode_to_html(child, out);
            }
        }
        VNode::Raw {
            tag,
            props,
            html,
            nl_close,
        } => {
            match tag {
                Some(tag) => {
                    out.push('<');
                    out.push_str(tag);
                    write_attrs(props, out);
                    out.push('>');
                    out.push_str(html);
                    out.push_str("</");
                    out.push_str(tag);
                    out.push('>');
                }
                None => out.push_str(html),
            }
            if *nl_close {
                out.push('\n');
            }
        }
        VNode::Element {
            tag,
            props,
            children,
            nl_open,
            nl_close,
            void,
        } => {
            if *void {
                out.push('<');
                out.push_str(tag);
                write_attrs(props, out);
                out.push_str(" />");
                if *nl_close {
                    out.push('\n');
                }
                return;
            }
            out.push('<');
            out.push_str(tag);
            write_attrs(props, out);
            out.push('>');
            if *nl_open {
                out.push('\n');
            }
            for child in children {
                vnode_to_html(child, out);
            }
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
            if *nl_close {
                out.push('\n');
            }
        }
    }
}

// [spec:mw:req:html.escape.attribute-contexts]
fn write_attrs(props: &Props, out: &mut String) {
    for (key, val) in props.iter() {
        match val {
            AttrVal::Esc(s) => {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                escape_html(s, out);
                out.push('"');
            }
            AttrVal::Url(s) => {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                escape_href(s, out);
                out.push('"');
            }
            AttrVal::Lit(s) => {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                out.push_str(s);
                out.push('"');
            }
            AttrVal::Flag => {
                out.push(' ');
                out.push_str(key);
            }
            AttrVal::Empty => {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"\"");
            }
        }
    }
}

// ----- streaming sink ----------------------------------------------

pub struct HtmlSink<S, R = ()> {
    inner: S,
    adapter: HtmlAdapter<R>,
}

impl<S> HtmlSink<S, ()> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            adapter: HtmlAdapter::new(),
        }
    }
}

impl<S, R> HtmlSink<S, R> {
    pub fn with_custom_renderer(inner: S, custom: R) -> Self {
        Self {
            inner,
            adapter: HtmlAdapter::with_custom_renderer(custom),
        }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, R, Ext> Sink<Node<Ext>> for HtmlSink<S, R>
where
    S: Sink<String>,
    S::Error: fmt::Display,
    R: RenderHtmlCustom<Ext>,
{
    type Error = HtmlError;

    fn emit(&mut self, item: StreamItem<Node<Ext>>) -> Result<(), Self::Error> {
        let rendered = self.adapter.render(item)?;
        self.inner
            .emit(rendered)
            .map_err(|error| HtmlError::Sink(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::adapter::html::{HtmlAdapter, HtmlSink, RenderHtmlCustom};
    use crate::node::{Emission, Node, SourceSpan, StreamItem};
    use crate::sink::{Sink, VecSink};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestExt {
        Math(String),
    }

    struct MathRenderer;

    impl RenderHtmlCustom<TestExt> for MathRenderer {
        fn render_custom(
            &self,
            payload: &TestExt,
            _children: &[Node<TestExt>],
            out: &mut String,
        ) -> Result<(), super::HtmlError> {
            match payload {
                TestExt::Math(source) => {
                    out.push_str("<span class=\"math\">");
                    out.push_str(source);
                    out.push_str("</span>");
                    Ok(())
                }
            }
        }
    }

    // [spec:mw:req:html.render.serializer-over-vdom/test]
    #[test]
    fn renders_html_upsert_with_same_id_and_provisional_flag() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 7,
                provisional: true,
                node: Node::<()>::Paragraph {
                    children: vec![Node::Text {
                        text: "hello".to_owned(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();

        assert_eq!(
            rendered,
            StreamItem::Upsert(Emission {
                id: 7,
                provisional: true,
                node: "<p>hello</p>\n".to_owned(),
            })
        );
    }

    // [spec:mw:req:html.render.serializer-over-vdom/test]
    #[test]
    fn passes_delete_through_unchanged() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::<Node<()>>::Delete { id: 9 })
            .unwrap();
        assert_eq!(rendered, StreamItem::Delete { id: 9 });
    }

    // [spec:mw:sem:html.serialize.emission/test]
    #[test]
    fn renders_lists_and_block_quotes() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 1,
                provisional: false,
                node: Node::<()>::BlockQuote {
                    children: vec![Node::List {
                        ordered: false,
                        start: None,
                        tight: None,
                        children: vec![Node::Item {
                            children: vec![Node::Paragraph {
                                children: vec![Node::Text {
                                    text: "item".to_owned(),
                                    source: SourceSpan::default(),
                                }],
                                source: SourceSpan::default(),
                            }],
                            source: SourceSpan::default(),
                        }],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();

        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert_eq!(
            emission.node,
            "<blockquote>\n<ul>\n<li>item</li>\n</ul>\n</blockquote>\n"
        );
    }

    // [spec:mw:def:html.adapter.surface/test]
    // [spec:mw:def:core.sink.trait/test]
    #[test]
    fn html_sink_wraps_existing_sink() {
        let inner = VecSink::<String>::new();
        let mut sink = HtmlSink::new(inner);

        sink.emit(StreamItem::Upsert(Emission {
            id: 11,
            provisional: false,
            node: Node::<()>::ThematicBreak {
                source: SourceSpan::default(),
            },
        }))
        .unwrap();

        let inner = sink.into_inner();
        assert_eq!(
            inner.items(),
            &[StreamItem::Upsert(Emission {
                id: 11,
                provisional: false,
                node: "<hr />\n".to_owned(),
            })]
        );
    }

    // [spec:mw:req:highlighting.error-fallback/test]
    // [spec:mw:req:html.escape.text/test]
    #[cfg(feature = "highlight")]
    #[test]
    fn highlighter_error_falls_back_to_plain_code() {
        struct FailingHighlighter;

        impl crate::SyntaxHighlighter for FailingHighlighter {
            fn highlight(
                &mut self,
                _language_hint: &str,
                _source: &str,
            ) -> Result<Vec<crate::SyntaxSpan>, crate::HighlightError> {
                Err(crate::HighlightError::new("provider unavailable"))
            }
        }

        let rendered = HtmlAdapter::new()
            .with_highlighter(FailingHighlighter)
            .render(StreamItem::Upsert(Emission {
                id: 12,
                provisional: false,
                node: Node::<()>::CodeBlock {
                    info: Some("rust".to_owned()),
                    literal: "<tag>".to_owned(),
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();

        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert!(
            emission.node.contains("&lt;tag&gt;"),
            "plain fallback lost or failed to escape source: {}",
            emission.node
        );
        assert!(
            !emission.node.contains("<a-"),
            "failed highlighting emitted styled markup: {}",
            emission.node
        );
    }

    // [spec:adv:req:markdown.url-allowlist/test]
    #[test]
    fn javascript_link_destination_is_stripped() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 1,
                provisional: false,
                node: Node::<()>::Paragraph {
                    children: vec![Node::Link {
                        destination: "javascript:alert(1)".to_owned(),
                        title: None,
                        children: vec![Node::Text {
                            text: "x".to_owned(),
                            source: SourceSpan::default(),
                        }],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();
        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert!(
            !emission.node.contains("javascript:"),
            "got: {}",
            emission.node
        );
        assert!(
            emission.node.contains("href=\"\""),
            "got: {}",
            emission.node
        );
    }

    #[test]
    fn javascript_image_destination_is_stripped() {
        let adapter = HtmlAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 1,
                provisional: false,
                node: Node::<()>::Paragraph {
                    children: vec![Node::Image {
                        destination: "javascript:alert(1)".to_owned(),
                        title: None,
                        alt: "x".to_owned(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();
        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert!(
            !emission.node.contains("javascript:"),
            "got: {}",
            emission.node
        );
    }

    // [spec:mw:def:html.custom.hook/test]
    // [spec:mw:sem:html.custom.bridge/test]
    #[test]
    fn custom_renderer_handles_custom_nodes() {
        let adapter = HtmlAdapter::<()>::with_custom_renderer(MathRenderer);
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 3,
                provisional: false,
                node: Node::<TestExt>::Paragraph {
                    children: vec![Node::Custom {
                        payload: TestExt::Math("x^2".to_owned()),
                        children: Vec::new(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();

        let StreamItem::Upsert(emission) = rendered else {
            panic!("expected upsert")
        };
        assert_eq!(emission.node, "<p><span class=\"math\">x^2</span></p>\n");
    }

    // [spec:mw:req:html.escape.attribute-contexts/test]
    #[test]
    fn each_attr_val_variant_selects_its_own_encoding() {
        use crate::adapter::vdom::{AttrVal, Props};

        let mut props = Props::new();
        props.set("data-esc", AttrVal::Esc("a<b>&\"c".to_owned()));
        props.set("href", AttrVal::Url("/p?a=1&b=2 x\"".to_owned()));
        props.set("class", AttrVal::Lit("a<b>&\"c".to_owned()));
        props.set("data-footnotes", AttrVal::Flag);
        props.set("disabled", AttrVal::Empty);

        let mut out = String::new();
        super::write_attrs(&props, &mut out);

        assert_eq!(
            out,
            concat!(
                // Esc: through the HTML escaper.
                " data-esc=\"a&lt;b&gt;&amp;&quot;c\"",
                // Url: through the percent-encoder, keeping `&` and `=`.
                " href=\"/p?a=1&b=2%20x%22\"",
                // Lit: copied with no transformation at all.
                " class=\"a<b>&\"c\"",
                // Flag: bare key, no `=`, no quotes.
                " data-footnotes",
                // Empty: key=""
                " disabled=\"\"",
            )
        );
    }

    // [spec:mw:req:html.escape.attribute-contexts/test]
    #[test]
    fn attributes_are_emitted_in_props_insertion_order() {
        use crate::adapter::vdom::{AttrVal, Props};

        let mut forward = Props::new();
        forward.set("a", AttrVal::Lit("1".to_owned()));
        forward.set("b", AttrVal::Lit("2".to_owned()));
        let mut reverse = Props::new();
        reverse.set("b", AttrVal::Lit("2".to_owned()));
        reverse.set("a", AttrVal::Lit("1".to_owned()));

        let mut forward_out = String::new();
        super::write_attrs(&forward, &mut forward_out);
        let mut reverse_out = String::new();
        super::write_attrs(&reverse, &mut reverse_out);

        assert_eq!(forward_out, " a=\"1\" b=\"2\"");
        assert_eq!(reverse_out, " b=\"2\" a=\"1\"");
    }

    // [spec:mw:req:html.escape.attribute-contexts/test]
    #[test]
    fn document_derived_attribute_values_reach_the_output_encoded() {
        // Whole-document check that the adapter routes each real attribute
        // through the variant the rule requires: `Esc` for the image title,
        // `Url` for its destination, `Lit` for the alignment class, `Flag`
        // for `data-footnotes`, `Empty` for the task checkbox.
        let mut parser = crate::Parser::<()>::builder().with_all().build();
        let blocks = parser
            .parse_full_at(
                "| a |\n|:-:|\n| 1 |\n\n\
                 - [ ] todo\n\n\
                 ![alt <b>](</img a.png> 'ti\"tle <b>')\n\n\
                 note[^1]\n\n[^1]: body\n",
                0,
            )
            .expect("parse");

        let adapter = HtmlAdapter::new();
        let mut html = String::new();
        for block in &blocks {
            let StreamItem::Upsert(emission) = adapter
                .render(StreamItem::Upsert(Emission {
                    id: 0,
                    provisional: false,
                    node: block.node.clone(),
                }))
                .unwrap()
            else {
                panic!("expected upsert")
            };
            html.push_str(&emission.node);
        }

        // Lit: the crate-controlled alignment class, verbatim.
        assert!(html.contains("align=\"center\""), "{html}");
        // Empty: valueless-with-quotes.
        assert!(html.contains("disabled=\"\""), "{html}");
        // Flag: bare key, no `=` and no quotes.
        assert!(html.contains(" data-footnotes>"), "{html}");
        assert!(!html.contains("data-footnotes=\""), "{html}");
        // Url: the space is percent-escaped, not HTML-escaped.
        assert!(html.contains("src=\"/img%20a.png\""), "{html}");
        // Esc: the title's quote and angle brackets are entity-escaped, so
        // they cannot terminate the attribute or open a tag.
        assert!(html.contains("title=\"ti&quot;tle &lt;b&gt;\""), "{html}");
        assert!(html.contains("alt=\"alt &lt;b&gt;\""), "{html}");
    }
}
