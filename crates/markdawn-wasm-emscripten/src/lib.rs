//! C ABI for driving the markdawn streaming parser from JavaScript.
//!
//! Lifecycle:
//!   1. `mw_state_new()` allocates a parser + accumulating HTML sink.
//!   2. `mw_state_push_byte(state, byte)` feeds one byte at a time.
//!      Bytes are buffered until they form valid UTF-8, then handed
//!      to `Parser::push_str`.
//!   3. `mw_state_push_chunk(state, ptr, len)` is the bulk variant.
//!   4. `mw_state_finish(state)` flushes any unfinalized blocks.
//!   5. `mw_state_render(state)` returns a freshly-allocated, NUL-
//!      terminated UTF-8 HTML snapshot.
//!   6. `mw_string_free(ptr)` frees a string returned from render.
//!   7. `mw_state_free(state)` tears the whole thing down.

use std::ffi::{CString, c_char};

use indexmap::IndexMap;
use markdawn::{CorrelationId, Emission, HtmlAdapter, HtmlError, Node, Parser, Sink, StreamItem};

struct HtmlPipeline {
    adapter: HtmlAdapter,
    blocks: IndexMap<CorrelationId, String>,
}

impl HtmlPipeline {
    fn new() -> Self {
        Self {
            adapter: HtmlAdapter::new(),
            blocks: IndexMap::new(),
        }
    }

    fn snapshot(&self) -> String {
        let total: usize = self.blocks.values().map(String::len).sum();
        let mut out = String::with_capacity(total);
        for chunk in self.blocks.values() {
            out.push_str(chunk);
        }
        out
    }
}

impl Sink<Node<()>> for HtmlPipeline {
    type Error = HtmlError;

    fn emit(&mut self, item: StreamItem<Node<()>>) -> Result<(), Self::Error> {
        match self.adapter.render(item)? {
            StreamItem::Upsert(Emission { id, node, .. }) => {
                self.blocks.insert(id, node);
            }
            StreamItem::Delete { id } => {
                self.blocks.shift_remove(&id);
            }
        }
        Ok(())
    }
}

// [spec:mw:def:ffi.ownership.handle]
pub struct State {
    parser: Parser<()>,
    pipeline: HtmlPipeline,
    pending: Vec<u8>,
}

impl State {
    fn new() -> Self {
        Self {
            parser: Parser::builder().with_all().build(),
            pipeline: HtmlPipeline::new(),
            pending: Vec::new(),
        }
    }

    // Bytes that don't yet form a complete code point stay buffered; a byte
    // that can never start one keeps the valid prefix at zero until `finish`.
    // [spec:mw:sem:ffi.encoding.utf8]
    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.pending.extend_from_slice(bytes);
        let valid_len = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(e) => e.valid_up_to(),
        };
        if valid_len == 0 {
            return Ok(());
        }
        let chunk = unsafe { std::str::from_utf8_unchecked(&self.pending[..valid_len]) }.to_owned();
        self.pending.drain(..valid_len);
        self.parser
            .push_str(&chunk, &mut self.pipeline)
            .map_err(|_| ())
    }

    fn push_byte(&mut self, byte: u8) -> Result<(), ()> {
        self.push_bytes(core::slice::from_ref(&byte))
    }

    fn finish(&mut self) -> Result<(), ()> {
        if !self.pending.is_empty() {
            if let Ok(s) = std::str::from_utf8(&self.pending) {
                let owned = s.to_owned();
                self.pending.clear();
                if self.parser.push_str(&owned, &mut self.pipeline).is_err() {
                    return Err(());
                }
            } else {
                self.pending.clear();
            }
        }
        self.parser.finish(&mut self.pipeline).map_err(|_| ())
    }

    fn render(&self) -> String {
        self.pipeline.snapshot()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mw_state_new() -> *mut State {
    Box::into_raw(Box::new(State::new()))
}

/// # Safety
/// `state` must be a pointer returned by `mw_state_new` and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_state_push_byte(state: *mut State, byte: u8) -> u32 {
    if state.is_null() {
        return 1;
    }
    let s = unsafe { &mut *state };
    if s.push_byte(byte).is_ok() { 0 } else { 1 }
}

/// # Safety
/// `state` must be a pointer returned by `mw_state_new`. `ptr` must
/// point to `len` readable bytes.
// The bytes live in the JS heap and are copied out before returning, so the
// caller can `_free` them as soon as the call completes.
// [spec:mw:req:ffi.ownership.borrowed-input]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_state_push_chunk(state: *mut State, ptr: *const u8, len: usize) -> u32 {
    if state.is_null() || (ptr.is_null() && len > 0) {
        return 1;
    }
    let bytes = if len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };
    let s = unsafe { &mut *state };
    if s.push_bytes(bytes).is_ok() { 0 } else { 1 }
}

/// # Safety
/// `state` must be a pointer returned by `mw_state_new` and not yet freed.
// Parser errors collapse to a status code rather than propagating out of the
// extern "C" frame.
// [spec:mw:req:ffi.safety.no-unwind]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_state_finish(state: *mut State) -> u32 {
    if state.is_null() {
        return 1;
    }
    let s = unsafe { &mut *state };
    if s.finish().is_ok() { 0 } else { 1 }
}

/// Caller owns the returned pointer and must release it via `mw_string_free`.
///
/// # Safety
/// `state` must be a pointer returned by `mw_state_new` and not yet freed.
// [spec:mw:req:ffi.ownership.string-free]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_state_render(state: *const State) -> *mut c_char {
    if state.is_null() {
        return core::ptr::null_mut();
    }
    let s = unsafe { &*state };
    match CString::new(s.render()) {
        Ok(c) => c.into_raw(),
        Err(_) => core::ptr::null_mut(),
    }
}

/// # Safety
/// `ptr` must be a pointer returned by `mw_state_render` and not yet freed.
// Frees against this module's heap only — a pointer minted by one of the
// lazily-loaded render modules must go back to that module's copy.
// [spec:mw:req:ffi.ownership.string-free]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(ptr) });
}

/// # Safety
/// `state` must be a pointer returned by `mw_state_new` and not yet freed.
// [spec:mw:req:ffi.ownership.handle-release]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_state_free(state: *mut State) {
    if state.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(state) });
}
