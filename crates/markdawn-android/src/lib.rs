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

mod grammar_store;

use std::ffi::{c_char, c_void};

use grammar_store::{global_store, mw_register_grammar};
use indexmap::IndexMap;
use markdawn::{self as md, Sink};
use serde::Serialize;

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jlong, jstring};

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
                .with_highlighter(md::arborium::Highlighter::with_store(
                    global_store().clone(),
                )),
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

/// `dlsym` a plugin's `mw_lang_<name>_init` (the plugin `.so` has already been
/// pulled into the process by Kotlin's `System.loadLibrary`) and call it,
/// handing over `mw_register_grammar` so it registers its grammar into the
/// global store. The JNI analogue of Swift's `Markdawn.loadLanguage` — but the
/// `dlsym`+call runs here in Rust, since Kotlin can't take the address of the
/// `mw_register_grammar` C symbol to pass across.
fn register_language(name: &str) -> bool {
    type RegisterFn = unsafe extern "C" fn(
        *const c_char,
        usize,
        *const c_void,
        *const c_char,
        usize,
        *const c_char,
        usize,
        *const c_char,
        usize,
    ) -> u32;
    type InitFn = unsafe extern "C" fn(RegisterFn) -> u32;

    let sanitized = name.replace('-', "_");
    let symbol = format!("mw_lang_{sanitized}_init\0");
    let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr() as *const c_char) };
    if sym.is_null() {
        return false;
    }
    let init_fn: InitFn = unsafe { std::mem::transmute(sym) };
    unsafe { init_fn(mw_register_grammar) == 0 }
}

// ===================================================================
// JNI surface — class `nu.necessary.markdawn.Native`
// ===================================================================

fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    if s.is_null() {
        return String::new();
    }
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn to_jstring(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .map(|o| o.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn throw(env: &mut JNIEnv, msg: &str) {
    let _ = env.throw_new("java/lang/RuntimeException", msg);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parseToVdom<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    source: JString<'local>,
    options: JString<'local>,
) -> jstring {
    let source = jstr(&mut env, &source);
    let options = jstr(&mut env, &options);
    match parse_to_vdom_json(&source, &options) {
        Ok(json) => to_jstring(&mut env, &json),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_loadLanguage<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
) -> jboolean {
    let name = jstr(&mut env, &name);
    if register_language(&name) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

// ----- streaming VdomParser (opaque pointer) -----------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserNew<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    options: JString<'local>,
) -> jlong {
    let options = jstr(&mut env, &options);
    Box::into_raw(Box::new(VdomParser::new(&options))) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserPush<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    chunk: JString<'local>,
) {
    let parser = unsafe { &mut *(ptr as *mut VdomParser) };
    let chunk = jstr(&mut env, &chunk);
    if let Err(e) = parser.push_str(&chunk) {
        throw(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserFinish<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) {
    let parser = unsafe { &mut *(ptr as *mut VdomParser) };
    if let Err(e) = parser.finish() {
        throw(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserTakeEvents<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) -> jstring {
    let parser = unsafe { &mut *(ptr as *mut VdomParser) };
    match parser.take_events() {
        Ok(json) => to_jstring(&mut env, &json),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_parserFree<'local>(
    _env: JNIEnv<'local>,
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
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    options: JString<'local>,
) -> jlong {
    let options = jstr(&mut env, &options);
    Box::into_raw(Box::new(VdomDocument::new(&options))) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentSetSource<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    source: JString<'local>,
) {
    let doc = unsafe { &mut *(ptr as *mut VdomDocument) };
    let source = jstr(&mut env, &source);
    if let Err(e) = doc.set_source(&source) {
        throw(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentEdit<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    start: jlong,
    end: jlong,
    replacement: JString<'local>,
) {
    let doc = unsafe { &mut *(ptr as *mut VdomDocument) };
    let replacement = jstr(&mut env, &replacement);
    if let Err(e) = doc.edit(start as usize, end as usize, &replacement) {
        throw(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentSetCursor<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ptr: jlong,
    byte_offset: jlong,
) {
    let doc = unsafe { &mut *(ptr as *mut VdomDocument) };
    // A negative offset clears the cursor (Kotlin passes -1 for null).
    let offset = if byte_offset < 0 {
        None
    } else {
        Some(byte_offset as usize)
    };
    if let Err(e) = doc.set_cursor(offset) {
        throw(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentTakeEvents<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    ptr: jlong,
) -> jstring {
    let doc = unsafe { &mut *(ptr as *mut VdomDocument) };
    match doc.take_events() {
        Ok(json) => to_jstring(&mut env, &json),
        Err(e) => {
            throw(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_nu_necessary_markdawn_Native_documentFree<'local>(
    _env: JNIEnv<'local>,
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

    /// Register the bundled rust grammar into the process-global store the
    /// same way `mw_register_grammar` does — but in-process, so the highlight
    /// path is exercised without a plugin `.so`.
    fn register_rust_grammar() {
        use markdawn::arborium::advanced::{CompiledGrammar, GrammarConfig};
        use std::sync::Arc;
        let config = GrammarConfig {
            language: arborium_rust::language().into(),
            highlights_query: arborium_rust::HIGHLIGHTS_QUERY,
            injections_query: arborium_rust::INJECTIONS_QUERY,
            locals_query: arborium_rust::LOCALS_QUERY,
        };
        let grammar = CompiledGrammar::new(config).expect("rust grammar compiles");
        global_store().insert("rust".to_owned(), Arc::new(grammar));
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
