//! C ABI for driving the markdawn streaming parser from Swift / C.
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

/// Length of the trailing run of `bytes` that is a truncated-but-still-valid
/// UTF-8 sequence — the only thing a chunked stream can usefully hold back.
/// `Utf8Error::error_len()` is what separates the two failure kinds: `None` is
/// a sequence cut short by the end of the input, `Some(n)` an `n`-byte
/// subsequence that can never complete and so has no reason to wait.
pub(crate) fn incomplete_suffix_len(bytes: &[u8]) -> usize {
    let mut offset = 0;
    loop {
        let err = match std::str::from_utf8(&bytes[offset..]) {
            Ok(_) => return 0,
            Err(e) => e,
        };
        match err.error_len() {
            None => return bytes.len() - offset - err.valid_up_to(),
            Some(n) => offset += err.valid_up_to() + n,
        }
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

    // Only a truncated-but-still-valid tail waits for the next push; anything
    // ahead of it decodes now, ill-formed sequences included.
    // [spec:mw:sem:ffi.encoding.utf8+1]
    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.pending.extend_from_slice(bytes);
        let split = self.pending.len() - incomplete_suffix_len(&self.pending);
        if split == 0 {
            return Ok(());
        }
        let chunk = String::from_utf8_lossy(&self.pending[..split]).into_owned();
        self.pending.drain(..split);
        self.parser
            .push_str(&chunk, &mut self.pipeline)
            .map_err(|_| ())
    }

    fn push_byte(&mut self, byte: u8) -> Result<(), ()> {
        self.push_bytes(core::slice::from_ref(&byte))
    }

    // [spec:mw:sem:ffi.encoding.utf8+1]
    fn finish(&mut self) -> Result<(), ()> {
        if !self.pending.is_empty() {
            let tail = String::from_utf8_lossy(&self.pending).into_owned();
            self.pending.clear();
            if self.parser.push_str(&tail, &mut self.pipeline).is_err() {
                return Err(());
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
// [spec:mw:req:ffi.safety.null-guard+1]
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
// The chunk is copied into the pending buffer before returning; the caller
// may free or reuse it immediately.
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
// [spec:mw:req:ffi.ownership.string-free+2]
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
// The Swift staticlib links one allocator into the host process and the grammar
// plugins it loads never mint a `*mut c_char`, so there is no second heap here
// to confuse this with — it keeps the unqualified name the published header
// declares.
// [spec:mw:req:ffi.ownership.string-free+2]
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

#[cfg(test)]
#[path = "html_ffi_tests.rs"]
mod ffi_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    // `incomplete_suffix_len` is the whole decision: hold back a sequence the
    // next push can complete, never one that can only ever be ill-formed.
    // [spec:mw:sem:ffi.encoding.utf8+1/test]
    #[test]
    fn only_a_completable_tail_is_held_back() {
        assert_eq!(incomplete_suffix_len(b""), 0);
        assert_eq!(incomplete_suffix_len("héllo".as_bytes()), 0);
        // Truncated two-, three- and four-byte leads all wait.
        assert_eq!(incomplete_suffix_len(&[0xC3]), 1);
        assert_eq!(incomplete_suffix_len(&[0xE2, 0x82]), 2);
        assert_eq!(incomplete_suffix_len(&[0xF0, 0x9F, 0x92]), 3);
        // A byte that can never begin a sequence waits for nothing.
        assert_eq!(incomplete_suffix_len(&[0xFF]), 0);
        assert_eq!(incomplete_suffix_len(&[0x80]), 0);
        // An ill-formed run followed by a genuine truncation: only the
        // truncation is held.
        assert_eq!(incomplete_suffix_len(&[0xFF, 0xFE, 0xE2, 0x82]), 2);
        // A completed sequence after an ill-formed run holds nothing.
        assert_eq!(incomplete_suffix_len(&[0xFF, 0xE2, 0x82, 0xAC]), 0);
    }

    // [spec:mw:sem:ffi.encoding.utf8+1/test]
    #[test]
    fn truncated_sequence_waits_for_the_next_push() {
        let mut state = State::new();
        state.push_bytes(&[0xE2, 0x82]).unwrap();
        assert_eq!(state.pending, vec![0xE2, 0x82], "a valid prefix must wait");
        state.push_bytes(&[0xAC]).unwrap();
        assert!(
            state.pending.is_empty(),
            "the completed sequence must drain"
        );
        state.finish().unwrap();
        assert!(state.render().contains('€'), "€ must survive the split");
    }

    // [spec:mw:sem:ffi.encoding.utf8+1/test]
    #[test]
    fn invalid_byte_does_not_stall_the_stream() {
        let mut state = State::new();
        state.push_byte(0xFF).unwrap();
        state.push_bytes(b"visible\n").unwrap();
        state.finish().unwrap();
        let html = state.render();
        assert!(
            html.contains("visible"),
            "text pushed after an invalid byte must still parse: {html}"
        );
        assert!(
            html.contains('\u{FFFD}'),
            "the invalid byte must surface as U+FFFD: {html}"
        );
    }

    // [spec:mw:sem:ffi.encoding.utf8+1/test]
    #[test]
    fn finish_replaces_a_truncated_tail() {
        let mut state = State::new();
        state.push_bytes(b"a").unwrap();
        state.push_bytes(&[0xE2, 0x82]).unwrap();
        state.finish().unwrap();
        assert!(
            state.render().contains('\u{FFFD}'),
            "a tail cut short must reach the parser as U+FFFD"
        );
    }

    // [spec:mw:req:ffi.safety.null-guard+1/test]
    #[test]
    fn null_handle_is_rejected_in_band() {
        assert_eq!(
            unsafe { mw_state_push_byte(core::ptr::null_mut(), b'x') },
            1
        );
        assert_eq!(
            unsafe { mw_state_push_chunk(core::ptr::null_mut(), b"x".as_ptr(), 1) },
            1
        );
        assert_eq!(unsafe { mw_state_finish(core::ptr::null_mut()) }, 1);
        assert!(unsafe { mw_state_render(core::ptr::null()) }.is_null());
        unsafe { mw_state_free(core::ptr::null_mut()) };
        unsafe { mw_string_free(core::ptr::null_mut()) };
    }

    // [spec:mw:req:ffi.safety.null-guard+1/test]
    #[test]
    fn empty_chunk_accepts_a_null_data_pointer() {
        let state = mw_state_new();
        assert_eq!(
            unsafe { mw_state_push_chunk(state, core::ptr::null(), 0) },
            0
        );
        assert_eq!(
            unsafe { mw_state_push_chunk(state, core::ptr::null(), 1) },
            1
        );
        unsafe { mw_state_free(state) };
    }

    // [spec:mw:req:ffi.ownership.string-free+2/test]
    #[test]
    fn rendered_string_round_trips_through_this_modules_free() {
        let state = mw_state_new();
        let src = b"# Title\n";
        assert_eq!(
            unsafe { mw_state_push_chunk(state, src.as_ptr(), src.len()) },
            0
        );
        assert_eq!(unsafe { mw_state_finish(state) }, 0);
        let ptr = unsafe { mw_state_render(state) };
        assert!(!ptr.is_null());
        let html = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        assert!(html.contains("<h1"), "got: {html}");
        unsafe { mw_string_free(ptr) };
        unsafe { mw_state_free(state) };
    }
}
