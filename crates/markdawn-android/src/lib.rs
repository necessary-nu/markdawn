//! JNI bindings for the markdawn vdom target — the Android / Jetpack Compose
//! frontend.
//!
//! This is the JNI sibling of `crates/markdawn-wasm-bindgen` (its vdom
//! section): the same `VdomPipeline` / `parseToVdom` / `VdomParser` /
//! `VdomDocument` surface, but every value crossing the boundary is a JSON
//! string serialized with `serde_json` (Kotlin deserializes via
//! kotlinx.serialization) instead of a `JsValue` via serde-wasm-bindgen.
//!
//! Grammar handling mirrors `crates/markdawn-swift`: the `.so` ships the
//! `highlight` machinery with zero baked grammars; per-language tree-sitter
//! grammars are dynamic plugin `.so`s (the `markdawn-swift-lang-*` cdylibs)
//! loaded at runtime via [`Java_nu_necessary_markdawn_Native_loadLanguage`].

mod core_tests;
mod grammar_store;
mod jni_surface_tests;

use grammar_store::{global_registry, register_language};
use indexmap::IndexMap;
use markdawn::{self as md, Sink};
use serde::Serialize;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::strings::JNIString;
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jlong, jstring};
use jni::{Env, EnvUnowned, jni_str};

// ===================================================================
// JNI-free core logic (host-testable without an emulator)
// ===================================================================

/// One keyed reconciliation event for a top-level block, matching the wasm
/// crate's `JsVdomEvent`: `{ "type": "upsert", id, vtree }` /
/// `{ "type": "delete", id }`.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum VdomEvent {
    Upsert {
        id: md::CorrelationId,
        vtree: md::VNode,
    },
    Delete {
        id: md::CorrelationId,
    },
}

/// Options accepted by every entry point, parsed from a JSON object string,
/// e.g. `{ "rawHtml": "unrestricted" }`. Absent / unparseable → safe defaults.
#[derive(serde::Deserialize, Default)]
struct VdomOptions {
    #[serde(rename = "rawHtml", default)]
    raw_html: Option<String>,
}

/// Resolve the raw-HTML filter from the options JSON. Anything but
/// `"unrestricted"` (including an empty / malformed string) → `safe()`.
fn filter_from_options(options_json: &str) -> md::HtmlFilter {
    let mode = serde_json::from_str::<VdomOptions>(options_json)
        .ok()
        .and_then(|o| o.raw_html);
    if mode.as_deref() == Some("unrestricted") {
        md::HtmlFilter::unrestricted()
    } else {
        md::HtmlFilter::safe()
    }
}

/// Streaming vdom accumulator: renders each emitted block to a `VNode` and
/// buffers the resulting upsert/delete events for Kotlin to drain. The
/// adapter's highlighter consults the process-global grammar store, so any
/// language registered via `loadLanguage` is highlighted.
struct VdomPipeline {
    adapter: md::VdomAdapter,
    events: Vec<VdomEvent>,
}

impl VdomPipeline {
    fn new(html_filter: md::HtmlFilter) -> Self {
        Self {
            adapter: md::VdomAdapter::new()
                .with_html_filter(html_filter)
                .with_pikchr()
                .with_highlighter(md::DynamicHighlighter::new(global_registry().clone())),
            events: Vec::new(),
        }
    }

    fn consume(&mut self, items: Vec<md::StreamItem<md::Node<()>>>) -> Result<(), md::VdomError> {
        for item in items {
            self.emit(item)?;
        }
        Ok(())
    }

    fn take_events_json(&mut self) -> Result<String, String> {
        let events = core::mem::take(&mut self.events);
        serde_json::to_string(&events).map_err(|e| e.to_string())
    }
}

impl md::Sink<md::Node<()>> for VdomPipeline {
    type Error = md::VdomError;

    fn emit(&mut self, item: md::StreamItem<md::Node<()>>) -> Result<(), Self::Error> {
        match self.adapter.render(item)? {
            md::StreamItem::Upsert(md::Emission { id, node, .. }) => {
                self.events.push(VdomEvent::Upsert { id, vtree: node });
            }
            md::StreamItem::Delete { id } => {
                self.events.push(VdomEvent::Delete { id });
            }
        }
        Ok(())
    }
}

/// Parse `source` in one shot and return a JSON array of top-level block vnode
/// trees (`VNode[]`). Collapses the provisional event stream into the final
/// ordered block list, exactly as the wasm `parseToVdom` does.
fn parse_to_vdom_json(source: &str, options_json: &str) -> Result<String, String> {
    let mut p = md::Parser::<()>::builder().with_all().build();
    let mut pipeline = VdomPipeline::new(filter_from_options(options_json));
    p.push_str(source, &mut pipeline)
        .map_err(|e| e.to_string())?;
    p.finish(&mut pipeline).map_err(|e| e.to_string())?;

    let mut blocks: IndexMap<md::CorrelationId, md::VNode> = IndexMap::new();
    for event in pipeline.events {
        match event {
            VdomEvent::Upsert { id, vtree } => {
                blocks.insert(id, vtree);
            }
            VdomEvent::Delete { id } => {
                blocks.shift_remove(&id);
            }
        }
    }
    let trees: Vec<md::VNode> = blocks.into_values().collect();
    serde_json::to_string(&trees).map_err(|e| e.to_string())
}

/// Streaming parser: push chunks, `finish`, drain reconciliation events.
// [spec:mw:def:ffi.ownership.handle]
struct VdomParser {
    parser: md::Parser<()>,
    pipeline: VdomPipeline,
}

impl VdomParser {
    fn new(options_json: &str) -> Self {
        Self {
            parser: md::Parser::<()>::builder().with_all().build(),
            pipeline: VdomPipeline::new(filter_from_options(options_json)),
        }
    }

    fn push_str(&mut self, chunk: &str) -> Result<(), String> {
        self.parser
            .push_str(chunk, &mut self.pipeline)
            .map_err(|e| e.to_string())
    }

    fn finish(&mut self) -> Result<(), String> {
        self.parser
            .finish(&mut self.pipeline)
            .map_err(|e| e.to_string())
    }

    fn take_events(&mut self) -> Result<String, String> {
        self.pipeline.take_events_json()
    }
}

/// Edit-aware document over `md::EditableDocument`, emitting vnode events.
struct VdomDocument {
    inner: md::EditableDocument<()>,
    pipeline: VdomPipeline,
}

impl VdomDocument {
    fn new(options_json: &str) -> Self {
        Self {
            inner: md::EditableDocument::new(md::Parser::<()>::builder().with_all().build()),
            pipeline: VdomPipeline::new(filter_from_options(options_json)),
        }
    }

    fn set_source(&mut self, source: &str) -> Result<(), String> {
        let events = self.inner.set_source(source).map_err(|e| e.to_string())?;
        self.pipeline.consume(events).map_err(|e| e.to_string())
    }

    fn edit(&mut self, start: usize, end: usize, replacement: &str) -> Result<(), String> {
        let events = self
            .inner
            .edit(start..end, replacement)
            .map_err(|e| e.to_string())?;
        self.pipeline.consume(events).map_err(|e| e.to_string())
    }

    fn set_cursor(&mut self, byte_offset: Option<usize>) -> Result<(), String> {
        let events = self.inner.set_cursor(byte_offset);
        self.pipeline.consume(events).map_err(|e| e.to_string())
    }

    fn take_events(&mut self) -> Result<String, String> {
        self.pipeline.take_events_json()
    }
}

/// Register a plugin's grammar. The plugin `.so` has already been pulled into
/// the process by Kotlin's `System.loadLibrary`; `grammar_store` resolves its
/// per-language provider symbol and validates the descriptor behind it.
///
/// The JNI analogue of Swift's `Markdawn.loadLanguage`. It stays in Rust
/// because the symbol lookup and the ABI decode both do, not — as before —
/// because Kotlin could not take the address of a C symbol to pass across.
/// That handoff no longer exists.
// [spec:mw:def:ffi.plugin.grammar-registration+1]
fn register_plugin_language(name: &str) -> bool {
    register_language(name) == 0
}

// ===================================================================
// JNI surface — class `nu.necessary.markdawn.Native`
// ===================================================================

/// Run an entry point's body against the `Env` behind its `EnvUnowned`.
///
/// Every body reports its failures in-band before returning, so the closure
/// handed to jni never fails and the error policy is never consulted. The
/// `_no_catch` variant keeps a `catch_unwind` out of the entry points, which
/// must not depend on one.
fn with_env<'local, T: Default>(
    mut env: EnvUnowned<'local>,
    body: impl FnOnce(&mut Env<'local>) -> T,
) -> T {
    env.with_env_no_catch(|env| Ok::<_, jni::errors::Error>(body(env)))
        .resolve::<ThrowRuntimeExAndDefault>()
}

// A null `JString` and an unconvertible one both degrade to "" — no failure
// path here can raise, so callers keep their own error reporting.
// [spec:mw:req:ffi.safety.null-guard+1]
fn jstr(env: &Env, s: &JString) -> String {
    if s.is_null() {
        return String::new();
    }
    s.try_to_string(env).unwrap_or_default()
}

fn throw(env: &mut Env, msg: &str) {
    let _ = env.throw_new(jni_str!("java/lang/RuntimeException"), JNIString::from(msg));
}

/// Reborrow a `jlong` handle. A zero handle is the one host mistake this
/// surface can catch; it raises the same `RuntimeException` every other JNI
/// failure raises, and the caller returns its own null/void failure value.
/// A non-zero handle is trusted — a stale one is still undefined behaviour.
///
/// # Safety
/// A non-zero `ptr` must be a live handle minted by the matching constructor.
// [spec:mw:req:ffi.safety.null-guard+1]
unsafe fn handle<'a, T>(env: &mut Env, ptr: jlong) -> Option<&'a mut T> {
    if ptr == 0 {
        throw(env, "null handle");
        return None;
    }
    Some(unsafe { &mut *(ptr as *mut T) })
}

// [spec:mw:def:ffi.host.jni]
#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parseToVdom<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    source: JString<'local>,
    options: JString<'local>,
) -> jstring {
    with_env(env, |env| {
        let source = jstr(env, &source);
        let options = jstr(env, &options);
        match parse_to_vdom_json(&source, &options) {
            Ok(json) => env.new_string(&json).unwrap_or_default(),
            Err(e) => {
                throw(env, &e);
                JString::default()
            }
        }
    })
    .into_raw()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_loadLanguage<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
) -> jboolean {
    let name = with_env(env, |env| jstr(env, &name));
    if register_plugin_language(&name) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

// ----- streaming VdomParser (opaque pointer) -----------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserNew<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    options: JString<'local>,
) -> jlong {
    let options = with_env(env, |env| jstr(env, &options));
    Box::into_raw(Box::new(VdomParser::new(&options))) as jlong
}

// A parse error becomes a pending Java exception, never an unwind through the
// JNI frame.
// [spec:mw:req:ffi.safety.no-unwind]
#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserPush<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    chunk: JString<'local>,
) {
    with_env(env, |env| {
        let Some(parser) = (unsafe { handle::<VdomParser>(env, ptr) }) else {
            return;
        };
        let chunk = jstr(env, &chunk);
        if let Err(e) = parser.push_str(&chunk) {
            throw(env, &e);
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserFinish<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) {
    with_env(env, |env| {
        let Some(parser) = (unsafe { handle::<VdomParser>(env, ptr) }) else {
            return;
        };
        if let Err(e) = parser.finish() {
            throw(env, &e);
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserTakeEvents<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) -> jstring {
    with_env(env, |env| {
        let Some(parser) = (unsafe { handle::<VdomParser>(env, ptr) }) else {
            return JString::default();
        };
        match parser.take_events() {
            Ok(json) => env.new_string(&json).unwrap_or_default(),
            Err(e) => {
                throw(env, &e);
                JString::default()
            }
        }
    })
    .into_raw()
}

// [spec:mw:req:ffi.ownership.handle-release]
#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserFree<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) {
    if ptr != 0 {
        unsafe { drop(Box::from_raw(ptr as *mut VdomParser)) };
    }
}

// ----- editor VdomDocument (opaque pointer) ------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentNew<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    options: JString<'local>,
) -> jlong {
    let options = with_env(env, |env| jstr(env, &options));
    Box::into_raw(Box::new(VdomDocument::new(&options))) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentSetSource<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    source: JString<'local>,
) {
    with_env(env, |env| {
        let Some(doc) = (unsafe { handle::<VdomDocument>(env, ptr) }) else {
            return;
        };
        let source = jstr(env, &source);
        if let Err(e) = doc.set_source(&source) {
            throw(env, &e);
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentEdit<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    start: jlong,
    end: jlong,
    replacement: JString<'local>,
) {
    with_env(env, |env| {
        let Some(doc) = (unsafe { handle::<VdomDocument>(env, ptr) }) else {
            return;
        };
        let replacement = jstr(env, &replacement);
        if let Err(e) = doc.edit(start as usize, end as usize, &replacement) {
            throw(env, &e);
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentSetCursor<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    byte_offset: jlong,
) {
    with_env(env, |env| {
        let Some(doc) = (unsafe { handle::<VdomDocument>(env, ptr) }) else {
            return;
        };
        // A negative offset clears the cursor (Kotlin passes -1 for null).
        let offset = if byte_offset < 0 {
            None
        } else {
            Some(byte_offset as usize)
        };
        if let Err(e) = doc.set_cursor(offset) {
            throw(env, &e);
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentTakeEvents<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) -> jstring {
    with_env(env, |env| {
        let Some(doc) = (unsafe { handle::<VdomDocument>(env, ptr) }) else {
            return JString::default();
        };
        match doc.take_events() {
            Ok(json) => env.new_string(&json).unwrap_or_default(),
            Err(e) => {
                throw(env, &e);
                JString::default()
            }
        }
    })
    .into_raw()
}

// [spec:mw:req:ffi.ownership.handle-release]
#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentFree<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) {
    if ptr != 0 {
        unsafe { drop(Box::from_raw(ptr as *mut VdomDocument)) };
    }
}

// ===================================================================
// Host contract tests (port of vdom_contract_test.ts) — no JNI/emulator.
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn blocks(json: &str) -> Vec<Value> {
        serde_json::from_str(json).unwrap()
    }

    /// Walk a vnode tree, returning the first node for which `pred` holds.
    fn find<'a>(node: &'a Value, pred: &dyn Fn(&Value) -> bool) -> Option<&'a Value> {
        if pred(node) {
            return Some(node);
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                if let Some(found) = find(child, pred) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn find_tag<'a>(trees: &'a [Value], tag: &str) -> Option<&'a Value> {
        trees
            .iter()
            .find_map(|t| find(t, &|n| n.get("tag").and_then(Value::as_str) == Some(tag)))
    }

    // [spec:mw:def:html.vdom.model/test]
    #[test]
    fn props_serialize_as_object() {
        let trees = blocks(&parse_to_vdom_json("[x](https://example.com)", "{}").unwrap());
        let a = find_tag(&trees, "a").expect("an <a> node");
        let props = a.get("props").expect("props key");
        assert!(
            props.is_object(),
            "props must be a JSON object, got {props}"
        );
        assert_eq!(
            props.get("href").and_then(Value::as_str),
            Some("https://example.com")
        );
    }

    // [spec:adv:req:markdown.url-allowlist/test]
    #[test]
    fn javascript_link_destination_is_neutralized() {
        let trees = blocks(&parse_to_vdom_json("[x](javascript:alert(1))", "{}").unwrap());
        let a = find_tag(&trees, "a").expect("an <a> node");
        let href = a
            .get("props")
            .and_then(|p| p.get("href"))
            .and_then(Value::as_str);
        assert_eq!(href, Some(""), "javascript: href must be empty");
    }

    // [spec:adv:req:markdown.url-allowlist/test]
    #[test]
    fn javascript_image_source_is_neutralized() {
        let trees = blocks(&parse_to_vdom_json("![x](javascript:alert(1))", "{}").unwrap());
        let img = find_tag(&trees, "img").expect("an <img> node");
        let src = img
            .get("props")
            .and_then(|p| p.get("src"))
            .and_then(Value::as_str);
        assert_eq!(src, Some(""), "javascript: src must be empty");
    }

    // Math reaches the host through the `Raw` variant, which the serde
    // projection emits as an `html` key.
    // [spec:mw:def:html.vdom.model/test]
    #[test]
    fn inline_math_is_raw_svg() {
        let trees = blocks(&parse_to_vdom_json("$x^2$", "{}").unwrap());
        let raw = trees
            .iter()
            .find_map(|t| find(t, &|n| n.get("html").is_some()))
            .expect("a node carrying raw html");
        let html = raw.get("html").and_then(Value::as_str).unwrap_or_default();
        assert!(
            html.contains("<svg"),
            "math should render to inline SVG, got: {html}"
        );
    }

    // A diagram is a `Raw` node carrying both `html` and `props`, both of
    // which the serde projection must surface to the host.
    // [spec:mw:def:html.vdom.model/test]
    #[test]
    fn pikchr_code_block_is_an_accessible_inline_svg() {
        let trees = blocks(&parse_to_vdom_json("```pikchr\nbox \"Hello\"\n```\n", "{}").unwrap());
        let raw = trees
            .iter()
            .find_map(|tree| find(tree, &|node| node.get("html").is_some()))
            .expect("a raw SVG node for the pikchr diagram");
        let html = raw.get("html").and_then(Value::as_str).unwrap_or_default();
        assert!(
            html.contains("<svg"),
            "pikchr should render to SVG, got: {html}"
        );
        assert_eq!(
            raw.pointer("/props/aria-label").and_then(Value::as_str),
            Some("Pikchr diagram")
        );
    }

    // Exercises the filter policy, not the JNI binding: `{}` must
    // sanitize and an explicit `unrestricted` must pass through.
    // [spec:mw:req:core.html-filter.default-safe/test]
    // [spec:mw:def:core.html-filter.modes/test]
    #[test]
    fn raw_html_toggle() {
        let src = "<script>alert(1)</script>\n";
        let safe = parse_to_vdom_json(src, "{}").unwrap();
        let unrestricted = parse_to_vdom_json(src, r#"{"rawHtml":"unrestricted"}"#).unwrap();
        assert!(
            !safe.contains("<script"),
            "safe mode must strip <script>, got: {safe}"
        );
        assert!(
            unrestricted.contains("<script"),
            "unrestricted mode must pass <script> through, got: {unrestricted}"
        );
    }

    /// Collapse a stream of upsert/delete events (the wire format) into the
    /// final ordered block list, the same way `parse_to_vdom_json` does.
    /// Returns parsed `Value`s so comparison is key-order-independent (the
    /// streamed path round-trips through `Value`, the one-shot path serializes
    /// `VNode` struct fields directly — same content, different key order).
    fn collapse_events(events_json: &str) -> Vec<Value> {
        let events: Vec<Value> = serde_json::from_str(events_json).unwrap();
        let mut map: IndexMap<u64, Value> = IndexMap::new();
        for ev in events {
            let ty = ev.get("type").and_then(Value::as_str).unwrap();
            let id = ev.get("id").and_then(Value::as_u64).unwrap();
            if ty == "upsert" {
                map.insert(id, ev.get("vtree").cloned().unwrap());
            } else {
                map.shift_remove(&id);
            }
        }
        map.into_values().collect()
    }

    // Collapsing the event stream by correlation id must reproduce the
    // one-shot render: position-keyed slot ids that stay put across
    // reparses, and upserts applied as create-or-replace.
    // [spec:mw:req:core.stream.slot-identity/test]
    // [spec:mw:req:core.stream.upsert-replaces/test]
    #[test]
    fn streaming_matches_static() {
        let src = "# Title\n\nA paragraph with **bold** and `code`.\n\n- one\n- two\n\n> a quote\n";
        let one_shot: Vec<Value> =
            serde_json::from_str(&parse_to_vdom_json(src, "{}").unwrap()).unwrap();

        // Feed the source in small char-boundary chunks through the streaming
        // parser, then collapse its events.
        let mut parser = VdomParser::new("{}");
        let chars: Vec<char> = src.chars().collect();
        for chunk in chars.chunks(4) {
            let piece: String = chunk.iter().collect();
            parser.push_str(&piece).unwrap();
        }
        parser.finish().unwrap();
        let streamed = collapse_events(&parser.take_events().unwrap());

        assert_eq!(
            streamed, one_shot,
            "streaming must converge to the one-shot render"
        );
    }

    /// Register the bundled rust grammar into the process-global registry
    /// with the same descriptor a plugin would hand over — but linked in, so
    /// the highlight path is exercised without a plugin `.so`.
    fn register_rust_grammar() {
        // Idempotent for the suite's sake: registration rejects a duplicate
        // canonical id, and more than one test needs a grammar present.
        let _ = global_registry().register(nts_lang_rust::DESCRIPTOR);
    }

    #[test]
    fn highlighted_code_emits_custom_element_html() {
        register_rust_grammar();
        let json = parse_to_vdom_json("```rust\nfn main() {}\n```\n", "{}").unwrap();
        // Captured for the Kotlin parser fixture:
        println!("HL_FIXTURE={json}");
        let trees = blocks(&json);
        let raw = trees
            .iter()
            .find_map(|t| find(t, &|n| n.get("html").is_some()))
            .expect("a Raw html node for the highlighted code");
        let html = raw.get("html").and_then(Value::as_str).unwrap_or_default();
        // CustomElements format: <a-k>fn</a-k> etc. — `k` = keyword.
        assert!(
            html.contains("<a-"),
            "expected custom-element spans, got: {html}"
        );
        assert!(
            html.contains("<a-k>"),
            "expected a keyword span, got: {html}"
        );
    }

    #[test]
    fn editor_edit_emits_events() {
        let mut doc = VdomDocument::new("{}");
        doc.set_source("hello\n\nworld\n").unwrap();
        let _ = doc.take_events().unwrap();
        // Replace "world" (after "hello\n\n" = bytes 7..12) with "there".
        doc.edit(7, 12, "there").unwrap();
        let events: Vec<Value> = serde_json::from_str(&doc.take_events().unwrap()).unwrap();
        assert!(
            !events.is_empty(),
            "editing a block must emit at least one reconciliation event"
        );
    }
}
