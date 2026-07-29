//! Extension-protocol, error-location and sink-abort contracts of the
//! core parser, exercised through the public API.

use markdawn::{
    BlockMatch, Emission, Error, ErrorLocation, Extension, InlineContext, InlineMatch, Node,
    Parser, Sink, SourceSpan, StreamError, StreamItem, VecSink,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ext {
    Tag(&'static str),
    Captured(Vec<Node<Ext>>),
}

fn custom(payload: Ext) -> Node<Ext> {
    Node::Custom {
        payload,
        children: Vec::new(),
        source: SourceSpan::default(),
    }
}

/// Returns an inline match with a caller-chosen `consumed` whenever the
/// remaining input starts with `@`.
struct AtSign(usize);

impl Extension<Ext> for AtSign {
    fn try_inline(&self, input: &str) -> Option<InlineMatch<Ext>> {
        input.starts_with('@').then(|| InlineMatch {
            consumed: self.0,
            node: custom(Ext::Tag("at")),
        })
    }
}

fn push_document(extension: impl Extension<Ext> + 'static, source: &str) -> Result<(), Error> {
    let mut parser = Parser::<Ext>::builder().with_extension(extension).build();
    let mut sink = VecSink::new();
    match parser.push_str(source, &mut sink) {
        Ok(()) => Ok(()),
        Err(StreamError::Parser(error)) => Err(error),
        Err(StreamError::Sink(never)) => match never {},
    }
}

fn invalid_match_offset(error: Error) -> usize {
    match error {
        Error::InvalidExtensionMatch {
            location: Some(ErrorLocation { byte_offset }),
        } => byte_offset,
        other => panic!("expected InvalidExtensionMatch with a location, got {other:?}"),
    }
}

// [spec:mw:req:core.ext.inline-match-validity/test]
#[test]
fn zero_length_inline_match_is_rejected() {
    let error =
        push_document(AtSign(0), "a@b\n").expect_err("zero-consumed match must be an error");
    assert_eq!(invalid_match_offset(error), 1);
}

// [spec:mw:req:core.ext.inline-match-validity/test]
#[test]
fn inline_match_running_past_the_input_is_rejected() {
    // The paragraph's inline input is "a@b" (3 bytes); consuming 99 from
    // offset 1 overruns it.
    let error = push_document(AtSign(99), "a@b\n").expect_err("overrunning match must be an error");
    assert_eq!(invalid_match_offset(error), 1);
}

// [spec:mw:req:core.ext.inline-match-validity/test]
#[test]
fn inline_match_landing_mid_codepoint_is_rejected() {
    // "@é" — 'é' occupies bytes 1..3, so consuming 2 from offset 0 lands
    // inside it. In range, but not on a char boundary.
    let error = push_document(AtSign(2), "@é\n").expect_err("mid-codepoint match must be an error");
    assert_eq!(invalid_match_offset(error), 0);
}

// [spec:mw:req:core.ext.inline-match-validity/test]
#[test]
fn a_bad_match_aborts_rather_than_being_clamped_or_skipped() {
    // If the parser clamped `consumed` to the input length, or skipped the
    // match and carried on, the push would succeed and emit a paragraph.
    let mut parser = Parser::<Ext>::builder().with_extension(AtSign(99)).build();
    let mut sink = VecSink::new();
    let result = parser.push_str("a@b\n", &mut sink);

    assert!(result.is_err());
    assert!(
        sink.items().is_empty(),
        "no block may be emitted from an aborted parse: {:?}",
        sink.items()
    );

    // A valid match at the same site parses cleanly, so the rejection is
    // attributable to `consumed`, not to the extension firing at all.
    let mut parser = Parser::<Ext>::builder().with_extension(AtSign(1)).build();
    let mut sink = VecSink::new();
    parser.push_str("a@b\n", &mut sink).expect("valid match");
    assert!(!sink.items().is_empty());
}

// ----- block extensions -------------------------------------------------

/// Claims the trigger line but reports consuming nothing.
struct ZeroBlock;

impl Extension<Ext> for ZeroBlock {
    fn try_block(&self, lines: &[String], start: usize) -> Option<BlockMatch<Ext>> {
        (lines[start] == "@@block").then(|| BlockMatch {
            consumed_lines: 0,
            node: custom(Ext::Tag("zero")),
        })
    }
}

/// Claims the same trigger line, consuming it.
struct OneBlock;

impl Extension<Ext> for OneBlock {
    fn try_block(&self, lines: &[String], start: usize) -> Option<BlockMatch<Ext>> {
        (lines[start] == "@@block").then(|| BlockMatch {
            consumed_lines: 1,
            node: custom(Ext::Tag("one")),
        })
    }
}

fn finished_nodes(mut parser: Parser<Ext>, source: &str) -> Vec<Node<Ext>> {
    let mut sink = VecSink::new();
    parser.push_str(source, &mut sink).expect("push");
    parser.finish(&mut sink).expect("finish");

    let mut latest: Vec<Option<Node<Ext>>> = Vec::new();
    for item in sink.into_items() {
        match item {
            StreamItem::Upsert(Emission { id, node, .. }) => {
                let index = id as usize;
                if latest.len() <= index {
                    latest.resize(index + 1, None);
                }
                latest[index] = Some(node);
            }
            StreamItem::Delete { id } => {
                if let Some(slot) = latest.get_mut(id as usize) {
                    *slot = None;
                }
            }
        }
    }
    latest.into_iter().flatten().collect()
}

fn tags(nodes: &[Node<Ext>]) -> Vec<&'static str> {
    nodes
        .iter()
        .filter_map(|node| match node {
            Node::Custom {
                payload: Ext::Tag(tag),
                ..
            } => Some(*tag),
            _ => None,
        })
        .collect()
}

// [spec:mw:req:core.ext.block-zero-consumed/test]
#[test]
fn zero_consuming_block_match_is_discarded_and_the_next_extension_runs() {
    // ZeroBlock is registered first and claims the line; because it
    // consumed nothing its match is dropped and OneBlock's match wins.
    let parser = Parser::<Ext>::builder()
        .with_extension(ZeroBlock)
        .with_extension(OneBlock)
        .build();
    let nodes = finished_nodes(parser, "@@block\n");
    assert_eq!(tags(&nodes), vec!["one"]);
}

// [spec:mw:req:core.ext.block-zero-consumed/test]
#[test]
fn a_zero_consuming_block_extension_does_not_wedge_the_block_loop() {
    // Nothing else claims the line, so it falls through to ordinary
    // markdown instead of looping forever on the same start index.
    let parser = Parser::<Ext>::builder().with_extension(ZeroBlock).build();
    let nodes = finished_nodes(parser, "@@block\nafter\n");
    assert!(tags(&nodes).is_empty(), "zero match must not emit a node");
    assert!(
        nodes.iter().any(|node| matches!(
            node,
            Node::Paragraph { children, .. }
                if children.iter().any(|child| matches!(
                    child,
                    Node::Text { text, .. } if text.contains("@@block")
                ))
        )),
        "trigger line must be parsed as ordinary markdown: {nodes:?}"
    );
}

// ----- inline-context isolation ----------------------------------------

/// Captures `!!…!!` and recursively parses the interior through the
/// `InlineContext`, storing the resulting nodes in its payload.
struct Capture;

impl Extension<Ext> for Capture {
    fn try_inline_ctx(
        &self,
        input: &str,
        ctx: &dyn InlineContext<Ext>,
    ) -> Option<InlineMatch<Ext>> {
        let rest = input.strip_prefix("!!")?;
        let end = rest.find("!!")?;
        Some(InlineMatch {
            consumed: 2 + end + 2,
            node: custom(Ext::Captured(ctx.parse_inline(&rest[..end]))),
        })
    }
}

fn captured(nodes: &[Node<Ext>]) -> Vec<Node<Ext>> {
    let mut found = None;
    for node in nodes {
        node.walk(&mut |n| {
            if let Node::Custom {
                payload: Ext::Captured(inner),
                ..
            } = n
            {
                found = Some(inner.clone());
            }
        });
    }
    found.expect("no captured payload found")
}

// [spec:mw:sem:core.ext.inline-context-isolation/test]
#[test]
fn nested_inline_parse_does_not_inherit_link_reference_definitions() {
    let source = "[foo]: /url\n\nOutside [foo] and inside !![foo]!!.\n";
    let parser = Parser::<Ext>::builder().with_extension(Capture).build();
    let nodes = finished_nodes(parser, source);

    // The definition does exist for the surrounding document.
    let mut outer_links = 0usize;
    for node in &nodes {
        node.walk(&mut |n| {
            if let Node::Link { destination, .. } = n
                && destination == "/url"
            {
                outer_links += 1;
            }
        });
    }
    assert_eq!(outer_links, 1, "outer [foo] should resolve to a link");

    // Inside the capture it does not: `[foo]` stays literal text.
    let inner = captured(&nodes);
    assert!(
        !inner.iter().any(|n| matches!(n, Node::Link { .. })),
        "nested parse must not resolve the outer definition: {inner:?}"
    );
    let text: String = inner
        .iter()
        .filter_map(|n| match n {
            Node::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "[foo]");
}

// [spec:mw:sem:core.ext.inline-context-isolation/test]
#[test]
fn nested_inline_parse_does_not_inherit_footnote_labels() {
    let source = "Outside [^n] and inside !![^n]!!.\n\n[^n]: body\n";
    let parser = Parser::<Ext>::builder()
        .with_gfm()
        .with_extension(Capture)
        .build();
    let nodes = finished_nodes(parser, source);

    let mut outer_refs = 0usize;
    for node in &nodes {
        node.walk(&mut |n| {
            if matches!(n, Node::FootnoteRef { .. }) {
                outer_refs += 1;
            }
        });
    }
    assert!(
        outer_refs >= 1,
        "outer [^n] should resolve to a footnote ref"
    );

    let inner = captured(&nodes);
    assert!(
        !inner.iter().any(|n| matches!(n, Node::FootnoteRef { .. })),
        "nested parse must not inherit footnote labels: {inner:?}"
    );
}

// [spec:mw:sem:core.ext.inline-context-isolation/test]
#[test]
fn a_failing_nested_inline_parse_yields_no_nodes_instead_of_propagating() {
    // `Capture` is registered first, so it consumes `!!…!!` at the outer
    // level; the interior is then re-parsed with `AtSign(0)` live, whose
    // zero-length match makes the nested parse fail.
    let parser = Parser::<Ext>::builder()
        .with_extension(Capture)
        .with_extension(AtSign(0))
        .build();
    let nodes = finished_nodes(parser, "text !!a@b!! more\n");

    // The outer parse succeeded — the failure did not propagate.
    assert!(!nodes.is_empty());
    assert!(
        captured(&nodes).is_empty(),
        "a failed nested parse must yield an empty node list"
    );
}

// ----- sink errors ------------------------------------------------------

/// Records every item it is handed and fails the `fail_on`-th emit
/// (1-based).
struct FlakySink {
    accepted: Vec<StreamItem<Node<Ext>>>,
    attempts: usize,
    fail_on: usize,
}

impl Sink<Node<Ext>> for FlakySink {
    type Error = &'static str;

    fn emit(&mut self, item: StreamItem<Node<Ext>>) -> Result<(), Self::Error> {
        self.attempts += 1;
        if self.attempts == self.fail_on {
            return Err("sink is full");
        }
        self.accepted.push(item);
        Ok(())
    }
}

// [spec:mw:req:core.sink.error-aborts/test]
#[test]
fn a_sink_error_aborts_the_batch_and_surfaces_as_stream_error_sink() {
    let source = "one\n\ntwo\n\nthree\n\nfour\n";
    // Baseline: how many events does a clean finishing parse emit?
    let mut parser = Parser::<Ext>::new();
    let mut good = VecSink::new();
    parser.push_str(source, &mut good).expect("push");
    parser.finish(&mut good).expect("finish");
    let clean_events = good.items().len();
    assert!(clean_events >= 4, "expected a multi-event batch");

    let mut parser = Parser::<Ext>::new();
    let mut flaky = FlakySink {
        accepted: Vec::new(),
        attempts: 0,
        fail_on: 2,
    };
    let error = parser
        .push_str(source, &mut flaky)
        .expect_err("sink failure must surface");

    match error {
        StreamError::Sink(message) => assert_eq!(message, "sink is full"),
        other => panic!("expected StreamError::Sink, got {other:?}"),
    }

    // Exactly one emit succeeded, the failing emit was attempted once,
    // and nothing after it was attempted: no retry, no buffering, no
    // skipping past the failure.
    assert_eq!(flaky.accepted.len(), 1);
    assert_eq!(flaky.attempts, 2);
    assert!(flaky.attempts < clean_events);
}

// [spec:mw:req:core.sink.error-aborts/test]
#[test]
fn a_sink_error_on_the_first_emit_lets_nothing_through() {
    let mut parser = Parser::<Ext>::new();
    let mut flaky = FlakySink {
        accepted: Vec::new(),
        attempts: 0,
        fail_on: 1,
    };
    let error = parser
        .push_str("one\n\ntwo\n\nthree\n", &mut flaky)
        .expect_err("sink failure must surface");
    assert!(matches!(error, StreamError::Sink(_)));
    assert!(flaky.accepted.is_empty());
    assert_eq!(flaky.attempts, 1);
}
