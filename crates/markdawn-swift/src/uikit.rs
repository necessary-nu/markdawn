//! UIKit ABI for driving the markdawn streaming parser from Swift / C.
//!
//! Returns live Objective-C objects (a managed `UIStackView` of compartments,
//! optionally a flattened `NSAttributedString`) instead of HTML strings.
//!
//! `mw_uikit_state_new` must be called on the main thread — it returns NULL
//! otherwise. All other functions are thread-safe in the sense that they
//! can be called from the main thread; calling them from other threads is
//! undefined behaviour (the underlying UIKit objects aren't thread-safe).

use std::ffi::c_void;

use objc2::MainThreadMarker;
use objc2::Message;
use objc2::rc::Retained;
use objc2_ui_kit::UIStackView;

use markdawn::{BlockSink, DynamicHighlighter, Parser, UIKitError};

use crate::grammar_store::global_registry;
use crate::html::incomplete_suffix_len;

// [spec:mw:def:ffi.ownership.handle]
pub struct UIKitState {
    parser: Parser<()>,
    sink: BlockSink<()>,
    pending: Vec<u8>,
}

impl UIKitState {
    fn new(mtm: MainThreadMarker) -> Self {
        // The highlighter re-reads the process-global registry per block, so a
        // language registered by a plugin loaded after this state was built is
        // still picked up (see crate::grammar_store). Languages not registered
        // fall through to plain monospace rendering. `with_pikchr` renders
        // fenced `pikchr` blocks as inline diagrams.
        let sink = BlockSink::new(mtm)
            .with_highlighter(DynamicHighlighter::new(global_registry().clone()))
            .with_pikchr();
        Self {
            parser: Parser::builder().with_all().build(),
            sink,
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
            .push_str(&chunk, &mut self.sink)
            .map_err(|_: markdawn::StreamError<UIKitError>| ())
    }

    // [spec:mw:sem:ffi.encoding.utf8+1]
    fn finish(&mut self) -> Result<(), ()> {
        if !self.pending.is_empty() {
            let tail = String::from_utf8_lossy(&self.pending).into_owned();
            self.pending.clear();
            if self.parser.push_str(&tail, &mut self.sink).is_err() {
                return Err(());
            }
        }
        self.parser.finish(&mut self.sink).map_err(|_| ())
    }
}

/// Allocate a new UIKit-renderer state on the main thread.
///
/// Returns NULL if called off the main thread.
// [spec:mw:req:ffi.safety.null-guard+1]
#[unsafe(no_mangle)]
pub extern "C" fn mw_uikit_state_new() -> *mut UIKitState {
    let Some(mtm) = MainThreadMarker::new() else {
        return core::ptr::null_mut();
    };
    Box::into_raw(Box::new(UIKitState::new(mtm)))
}

/// # Safety
/// `state` must be a pointer returned by `mw_uikit_state_new`. `ptr` must
/// point to `len` readable bytes.
// The chunk is copied into the pending buffer before returning; the caller
// may free or reuse it immediately.
// [spec:mw:req:ffi.ownership.borrowed-input+1]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_uikit_state_push_chunk(
    state: *mut UIKitState,
    ptr: *const u8,
    len: usize,
) -> u32 {
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
/// `state` must be a pointer returned by `mw_uikit_state_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_uikit_state_finish(state: *mut UIKitState) -> u32 {
    if state.is_null() {
        return 1;
    }
    let s = unsafe { &mut *state };
    if s.finish().is_ok() { 0 } else { 1 }
}

/// Returns a +1 retained `UIStackView*`. The caller takes ownership; in Swift
/// use `Unmanaged.fromOpaque(ptr).takeRetainedValue()`. The sink keeps its own
/// reference for continued in-place updates, so embedding the returned view in
/// a hierarchy and continuing to push chunks both work.
///
/// # Safety
/// `state` must be a pointer returned by `mw_uikit_state_new`.
// [spec:mw:req:ffi.ownership.retained-objc]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_uikit_state_stack_view(state: *const UIKitState) -> *mut c_void {
    if state.is_null() {
        return core::ptr::null_mut();
    }
    let s = unsafe { &*state };
    let retained: Retained<UIStackView> = s.sink.stack_view().retain();
    Retained::into_raw(retained).cast()
}

/// Returns a +1 retained `NSAttributedString*` snapshot of the whole document.
/// Useful for putting the rendered text into a single UILabel/UITextView.
///
/// # Safety
/// `state` must be a pointer returned by `mw_uikit_state_new`.
// [spec:mw:req:ffi.ownership.retained-objc]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_uikit_state_attributed_text(state: *const UIKitState) -> *mut c_void {
    if state.is_null() {
        return core::ptr::null_mut();
    }
    let s = unsafe { &*state };
    Retained::into_raw(s.sink.attributed_text()).cast()
}

/// # Safety
/// `state` must be a pointer returned by `mw_uikit_state_new` and not yet freed.
// [spec:mw:req:ffi.ownership.handle-release]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_uikit_state_free(state: *mut UIKitState) {
    if state.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(state) });
}
