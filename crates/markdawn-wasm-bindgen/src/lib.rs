//! wasm-bindgen bindings for the markdawn public API.
//!
//! This is the "rich" JS surface — every meaningful public type from
//! `markdawn`'s `lib.rs` `pub use` block is mirrored here as a
//! `#[wasm_bindgen]` class or free function. Generic types are
//! monomorphized at `Ext = ()`; trait-shaped APIs (`Sink<N>`,
//! `Extension<Ext>`) aren't expressible to JS and are exposed via
//! concrete owning wrappers instead.

use indexmap::IndexMap;
use markdawn::{self as md, Sink};
use wasm_bindgen::prelude::*;

// ----- Helpers -----------------------------------------------------

fn js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsError::new(&e.to_string()).into()
}

/// Combined HtmlAdapter + per-block accumulator. The streaming parser
/// can feed it directly (`Sink<Node<()>>`); `snapshot()` reifies the
/// current state into a single HTML string. Provisional in-progress
/// blocks are included because the parser re-emits them with the
/// same correlation id as input grows.
struct HtmlPipeline {
    adapter: md::HtmlAdapter,
    blocks: IndexMap<md::CorrelationId, String>,
}

impl HtmlPipeline {
    fn new(adapter: md::HtmlAdapter) -> Self {
        Self {
            adapter,
            blocks: IndexMap::new(),
        }
    }

    fn snapshot(&self) -> String {
        let mut out = String::with_capacity(self.blocks.values().map(String::len).sum());
        for chunk in self.blocks.values() {
            out.push_str(chunk);
        }
        out
    }

    fn consume(&mut self, items: Vec<md::StreamItem<md::Node<()>>>) -> Result<(), md::HtmlError> {
        for item in items {
            self.emit(item)?;
        }
        Ok(())
    }
}

impl md::Sink<md::Node<()>> for HtmlPipeline {
    type Error = md::HtmlError;

    fn emit(&mut self, item: md::StreamItem<md::Node<()>>) -> Result<(), Self::Error> {
        match self.adapter.render(item)? {
            md::StreamItem::Upsert(md::Emission { id, node, .. }) => {
                self.blocks.insert(id, node);
            }
            md::StreamItem::Delete { id } => {
                self.blocks.shift_remove(&id);
            }
        }
        Ok(())
    }
}

// ----- Top-level convenience functions -----------------------------

/// Parse `source` and render it to HTML in one shot. Equivalent to
/// `Parser::new()` + `push_str(source)` + `finish()` with a default
/// `HtmlAdapter`.
#[wasm_bindgen(js_name = parseToHtml)]
pub fn parse_to_html(source: &str) -> Result<String, JsValue> {
    let mut p = md::Parser::<()>::builder().with_all().build();
    let mut pipeline = HtmlPipeline::new(md::HtmlAdapter::new());
    p.push_str(source, &mut pipeline).map_err(js_err)?;
    p.finish(&mut pipeline).map_err(js_err)?;
    Ok(pipeline.snapshot())
}

/// Round-trip: parse `source` to a node tree, then serialize it back
/// to markdown. Useful for normalising user input or as a sanity
/// check on parser/serializer agreement.
#[wasm_bindgen(js_name = serializeMarkdown)]
pub fn serialize_markdown(source: &str) -> Result<String, JsValue> {
    let mut p = md::Parser::<()>::builder().with_all().build();
    let mut sink = md::VecSink::<md::Node<()>>::new();
    p.push_str(source, &mut sink)
        .map_err(|e| js_err(format!("{e}")))?;
    p.finish(&mut sink).map_err(|e| js_err(format!("{e}")))?;
    let nodes: Vec<md::Node<()>> = sink
        .into_items()
        .into_iter()
        .filter_map(|item| match item {
            md::StreamItem::Upsert(e) => Some(e.node),
            md::StreamItem::Delete { .. } => None,
        })
        .collect();
    Ok(md::serialize_blocks(&nodes))
}

// ----- ParserBuilder + Parser --------------------------------------

#[wasm_bindgen(js_name = ParserBuilder)]
pub struct JsParserBuilder {
    inner: md::ParserBuilder<()>,
}

#[wasm_bindgen(js_class = ParserBuilder)]
impl JsParserBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: md::ParserBuilder::new(),
        }
    }

    #[wasm_bindgen(js_name = withGfm)]
    pub fn with_gfm(mut self) -> Self {
        self.inner = self.inner.with_gfm();
        self
    }

    #[wasm_bindgen(js_name = withMath)]
    pub fn with_math(mut self) -> Self {
        self.inner = self.inner.with_math();
        self
    }

    #[wasm_bindgen(js_name = withAlerts)]
    pub fn with_alerts(mut self) -> Self {
        self.inner = self.inner.with_alerts();
        self
    }

    #[wasm_bindgen(js_name = withHighlights)]
    pub fn with_highlights(mut self) -> Self {
        self.inner = self.inner.with_highlights();
        self
    }

    #[wasm_bindgen(js_name = withDefinitionLists)]
    pub fn with_definition_lists(mut self) -> Self {
        self.inner = self.inner.with_definition_lists();
        self
    }

    #[wasm_bindgen(js_name = withAll)]
    pub fn with_all(mut self) -> Self {
        self.inner = self.inner.with_all();
        self
    }

    #[wasm_bindgen(js_name = withMaxInputBytes)]
    pub fn with_max_input_bytes(mut self, max: usize) -> Self {
        self.inner = self.inner.with_max_input_bytes(max);
        self
    }

    pub fn build(self) -> JsParser {
        JsParser::from_inner(self.inner.build())
    }
}

impl Default for JsParserBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming parser. Hold a `Parser` instance, push markdown in
/// chunks via `pushStr`, then `finish()` to flush the final block.
/// Call `renderHtml()` at any time to read the accumulating HTML
/// (provisional in-progress blocks included).
#[wasm_bindgen(js_name = Parser)]
pub struct JsParser {
    parser: md::Parser<()>,
    pipeline: HtmlPipeline,
}

#[wasm_bindgen(js_class = Parser)]
impl JsParser {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::from_inner(md::Parser::<()>::builder().with_all().build())
    }

    /// Push a chunk of markdown source through the streaming parser.
    /// Emitted blocks are rendered to HTML by an internal `HtmlAdapter`
    /// and accumulated by correlation id.
    #[wasm_bindgen(js_name = pushStr)]
    pub fn push_str(&mut self, chunk: &str) -> Result<(), JsValue> {
        self.parser
            .push_str(chunk, &mut self.pipeline)
            .map_err(|e| js_err(format!("{e}")))
    }

    /// Flush pending state. After `finish()` no more `pushStr` calls
    /// are allowed on this instance.
    pub fn finish(&mut self) -> Result<(), JsValue> {
        self.parser
            .finish(&mut self.pipeline)
            .map_err(|e| js_err(format!("{e}")))
    }

    /// Snapshot of the HTML rendered so far. Cheap to call repeatedly.
    #[wasm_bindgen(js_name = renderHtml)]
    pub fn render_html(&self) -> String {
        self.pipeline.snapshot()
    }
}

impl JsParser {
    fn from_inner(parser: md::Parser<()>) -> Self {
        Self {
            parser,
            pipeline: HtmlPipeline::new(md::HtmlAdapter::new()),
        }
    }
}

impl Default for JsParser {
    fn default() -> Self {
        Self::new()
    }
}

// ----- EditableDocument --------------------------------------------

/// Edit-aware wrapper over the parser. Maintains a source buffer and
/// re-parses incrementally when you call `setSource` or `edit`. The
/// `renderHtml()` snapshot includes provisional blocks the same way
/// the streaming parser does.
#[wasm_bindgen(js_name = EditableDocument)]
pub struct JsEditableDocument {
    inner: md::EditableDocument<()>,
    pipeline: HtmlPipeline,
}

#[wasm_bindgen(js_class = EditableDocument)]
impl JsEditableDocument {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::with_parser(md::Parser::<()>::builder().with_all().build())
    }

    /// Replace the entire buffer and re-parse from scratch.
    #[wasm_bindgen(js_name = setSource)]
    pub fn set_source(&mut self, source: &str) -> Result<(), JsValue> {
        let events = self
            .inner
            .set_source(source)
            .map_err(|e| js_err(format!("{e}")))?;
        self.pipeline.consume(events).map_err(js_err)
    }

    /// Splice `replacement` into the source at byte range `[start..end)`
    /// and re-parse only the affected blocks.
    pub fn edit(&mut self, start: usize, end: usize, replacement: &str) -> Result<(), JsValue> {
        let events = self
            .inner
            .edit(start..end, replacement)
            .map_err(|e| js_err(format!("{e}")))?;
        self.pipeline.consume(events).map_err(js_err)
    }

    /// Move the "cursor" (Bear-style reveal-source-under-cursor). Pass
    /// a byte offset, or `null`/`undefined` to clear.
    #[wasm_bindgen(js_name = setCursor)]
    pub fn set_cursor(&mut self, byte_offset: Option<usize>) -> Result<(), JsValue> {
        let events = self.inner.set_cursor(byte_offset);
        self.pipeline.consume(events).map_err(js_err)
    }

    /// Current source buffer.
    pub fn source(&self) -> String {
        self.inner.source().to_owned()
    }

    /// HTML snapshot.
    #[wasm_bindgen(js_name = renderHtml)]
    pub fn render_html(&self) -> String {
        self.pipeline.snapshot()
    }
}

impl JsEditableDocument {
    fn with_parser(parser: md::Parser<()>) -> Self {
        Self {
            inner: md::EditableDocument::new(parser),
            pipeline: HtmlPipeline::new(md::HtmlAdapter::new()),
        }
    }
}

impl Default for JsEditableDocument {
    fn default() -> Self {
        Self::new()
    }
}

// ----- URL parsing -------------------------------------------------

/// Parse a `markdawn:` scheme URL. Returns `null` if the input isn't
/// a markdawn URL. The returned object is plain JSON: `{ kind, ... }`.
#[wasm_bindgen(js_name = parseMarkdawnUrl)]
pub fn parse_markdawn_url(url: &str) -> Option<String> {
    md::parse_markdawn_url(url).map(|u| format!("{u:?}"))
}

// ----- Vdom (Preact / React / Solid frontend) ----------------------
//
// The vdom target hands JS a framework-neutral `VNode` tree per block
// instead of an HTML string. A thin `preact.js` wrapper maps each node
// to a Preact `h()` call. Top-level blocks are keyed by the parser's
// `CorrelationId`, so streaming/editor deltas reconcile in place.

use serde::Serialize;

/// Serialize to a JS value with maps rendered as plain objects. Preact
/// props must be objects, not `Map`s — serde-wasm-bindgen defaults to
/// `Map`, so the `VNode` `props` map needs this.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .map_err(js_err)
}

/// One keyed reconciliation event for a top-level block. `upsert`
/// carries the (re-)rendered vnode tree; `delete` drops the block.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum JsVdomEvent {
    Upsert {
        id: md::CorrelationId,
        vtree: md::VNode,
    },
    Delete {
        id: md::CorrelationId,
    },
}

/// Options accepted by the vdom entry points, e.g.
/// `{ rawHtml: "unrestricted" }`. Absent / unknown → safe defaults.
#[derive(serde::Deserialize, Default)]
struct VdomOptions {
    #[serde(rename = "rawHtml", default)]
    raw_html: Option<String>,
}

/// Resolve the raw-HTML filter from a JS options value. `undefined` /
/// `null` / anything but `"unrestricted"` → `safe()` (sanitized).
fn filter_from_options(options: &JsValue) -> md::HtmlFilter {
    let mode = if options.is_undefined() || options.is_null() {
        None
    } else {
        serde_wasm_bindgen::from_value::<VdomOptions>(options.clone())
            .ok()
            .and_then(|o| o.raw_html)
    };
    if mode.as_deref() == Some("unrestricted") {
        md::HtmlFilter::unrestricted()
    } else {
        md::HtmlFilter::safe()
    }
}

/// Streaming vdom accumulator: renders each emitted block to a `VNode`
/// and buffers the resulting upsert/delete events for JS to drain via
/// `takeEvents()`.
struct VdomPipeline {
    adapter: md::VdomAdapter,
    events: Vec<JsVdomEvent>,
}

impl VdomPipeline {
    fn new(html_filter: md::HtmlFilter) -> Self {
        Self {
            // No diagram engines — pikchr/typst (and their fonts) are
            // lazily-loaded wasm modules. Diagram blocks come through as
            // `<pre><code class="language-…">` and the frontend swaps in
            // the rendered SVG on demand.
            adapter: md::VdomAdapter::new().with_html_filter(html_filter),
            events: Vec::new(),
        }
    }

    fn consume(&mut self, items: Vec<md::StreamItem<md::Node<()>>>) -> Result<(), md::VdomError> {
        for item in items {
            self.emit(item)?;
        }
        Ok(())
    }

    fn take_events(&mut self) -> Result<JsValue, JsValue> {
        let events = core::mem::take(&mut self.events);
        to_js(&events)
    }
}

impl md::Sink<md::Node<()>> for VdomPipeline {
    type Error = md::VdomError;

    fn emit(&mut self, item: md::StreamItem<md::Node<()>>) -> Result<(), Self::Error> {
        match self.adapter.render(item)? {
            md::StreamItem::Upsert(md::Emission { id, node, .. }) => {
                self.events.push(JsVdomEvent::Upsert { id, vtree: node });
            }
            md::StreamItem::Delete { id } => {
                self.events.push(JsVdomEvent::Delete { id });
            }
        }
        Ok(())
    }
}

/// Parse `source` in one shot and return an array of top-level block
/// vnode trees (`VNode[]`). For static rendering; map each element to a
/// keyed child via `preact.js`.
#[wasm_bindgen(js_name = parseToVdom)]
pub fn parse_to_vdom(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    let mut p = md::Parser::<()>::builder().with_all().build();
    let mut pipeline = VdomPipeline::new(filter_from_options(&options));
    p.push_str(source, &mut pipeline).map_err(js_err)?;
    p.finish(&mut pipeline).map_err(js_err)?;

    // Collapse the event stream (which includes provisional re-emits)
    // into the final ordered block list.
    let mut blocks: IndexMap<md::CorrelationId, md::VNode> = IndexMap::new();
    for event in pipeline.events {
        match event {
            JsVdomEvent::Upsert { id, vtree } => {
                blocks.insert(id, vtree);
            }
            JsVdomEvent::Delete { id } => {
                blocks.shift_remove(&id);
            }
        }
    }
    let trees: Vec<md::VNode> = blocks.into_values().collect();
    to_js(&trees)
}

/// Streaming vdom parser. Push markdown in chunks via `pushStr`, then
/// `finish()`; drain reconciliation events with `takeEvents()` after
/// each chunk. Ideal for rendering streamed LLM/network output.
#[wasm_bindgen(js_name = VdomParser)]
pub struct JsVdomParser {
    parser: md::Parser<()>,
    pipeline: VdomPipeline,
}

#[wasm_bindgen(js_class = VdomParser)]
impl JsVdomParser {
    #[wasm_bindgen(constructor)]
    pub fn new(options: JsValue) -> Self {
        Self {
            parser: md::Parser::<()>::builder().with_all().build(),
            pipeline: VdomPipeline::new(filter_from_options(&options)),
        }
    }

    #[wasm_bindgen(js_name = pushStr)]
    pub fn push_str(&mut self, chunk: &str) -> Result<(), JsValue> {
        self.parser
            .push_str(chunk, &mut self.pipeline)
            .map_err(|e| js_err(format!("{e}")))
    }

    pub fn finish(&mut self) -> Result<(), JsValue> {
        self.parser
            .finish(&mut self.pipeline)
            .map_err(|e| js_err(format!("{e}")))
    }

    /// Drain the reconciliation events accumulated since the last call:
    /// `[{ type: "upsert", id, vtree } | { type: "delete", id }]`.
    #[wasm_bindgen(js_name = takeEvents)]
    pub fn take_events(&mut self) -> Result<JsValue, JsValue> {
        self.pipeline.take_events()
    }
}

impl Default for JsVdomParser {
    fn default() -> Self {
        Self::new(JsValue::UNDEFINED)
    }
}

/// Edit-aware vdom document. Mirrors `EditableDocument` but emits vnode
/// reconciliation events instead of HTML. Drive a live editor with
/// `setSource`/`edit`/`setCursor`, then `takeEvents()`.
#[wasm_bindgen(js_name = VdomDocument)]
pub struct JsVdomDocument {
    inner: md::EditableDocument<()>,
    pipeline: VdomPipeline,
}

#[wasm_bindgen(js_class = VdomDocument)]
impl JsVdomDocument {
    #[wasm_bindgen(constructor)]
    pub fn new(options: JsValue) -> Self {
        Self {
            inner: md::EditableDocument::new(md::Parser::<()>::builder().with_all().build()),
            pipeline: VdomPipeline::new(filter_from_options(&options)),
        }
    }

    #[wasm_bindgen(js_name = setSource)]
    pub fn set_source(&mut self, source: &str) -> Result<(), JsValue> {
        let events = self
            .inner
            .set_source(source)
            .map_err(|e| js_err(format!("{e}")))?;
        self.pipeline.consume(events).map_err(js_err)
    }

    pub fn edit(&mut self, start: usize, end: usize, replacement: &str) -> Result<(), JsValue> {
        let events = self
            .inner
            .edit(start..end, replacement)
            .map_err(|e| js_err(format!("{e}")))?;
        self.pipeline.consume(events).map_err(js_err)
    }

    #[wasm_bindgen(js_name = setCursor)]
    pub fn set_cursor(&mut self, byte_offset: Option<usize>) -> Result<(), JsValue> {
        let events = self.inner.set_cursor(byte_offset);
        self.pipeline.consume(events).map_err(js_err)
    }

    pub fn source(&self) -> String {
        self.inner.source().to_owned()
    }

    #[wasm_bindgen(js_name = takeEvents)]
    pub fn take_events(&mut self) -> Result<JsValue, JsValue> {
        self.pipeline.take_events()
    }
}

impl Default for JsVdomDocument {
    fn default() -> Self {
        Self::new(JsValue::UNDEFINED)
    }
}
