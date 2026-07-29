//! Lazy-loaded graphviz renderer module for the wasm demo.
//!
//! The main `markdawn-wasm-emscripten` build does not bundle graphviz.
//! When the rendered HTML contains `<code class="language-dot">` (or
//! `language-graphviz`), the JS post-processor dynamic-imports this
//! module and calls `mw_render_dot` to convert the DOT source to SVG,
//! then replaces the code block in place.

use std::ffi::{CString, c_char};

/// Render a DOT source string to SVG. Returns a newly-allocated,
/// NUL-terminated UTF-8 string the caller must release via
/// `mw_render_graphviz_string_free`. Returns null on render failure (the caller
/// should leave the original code block in place).
///
/// # Safety
/// `ptr` must point to `len` readable bytes of UTF-8.
// [spec:mw:sem:ffi.render-shim.lazy-module+1]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_render_dot(ptr: *const u8, len: usize) -> *mut c_char {
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    let Ok(source) = std::str::from_utf8(bytes) else {
        return core::ptr::null_mut();
    };
    let Ok(svg) = markdawn_graphviz::render_dot_to_svg(source) else {
        return core::ptr::null_mut();
    };
    match CString::new(svg) {
        Ok(c) => c.into_raw(),
        Err(_) => core::ptr::null_mut(),
    }
}

/// # Safety
/// `ptr` must be a pointer returned by *this* module's `mw_render_dot` and not
/// yet freed. This is this module's allocator over this module's linear memory;
/// a pointer minted by the core module or by another `markdawn-render-*` module
/// is an offset into a different heap, and nothing here can tell the two apart
/// — null is the only argument this function can reject. The engine-qualified
/// name is the guard: no other module exports it, so naming it against the
/// wrong module fails to resolve rather than freeing into the wrong heap.
// [spec:mw:req:ffi.ownership.string-free+2]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_render_graphviz_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(ptr) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    // The half of the ownership contract a single module can be held to: what
    // `mw_render_dot` mints, this module's engine-qualified free takes back,
    // and null passes through untouched. The cross-module half is not testable
    // from here — it needs two loaded wasm modules, and is not checkable at
    // runtime at all; the link-time guard is the engine-qualified name itself.
    // [spec:mw:req:ffi.ownership.string-free+2/test]
    #[test]
    fn rendered_string_round_trips_through_this_modules_free() {
        let src = "digraph { a -> b }";
        let ptr = unsafe { mw_render_dot(src.as_ptr(), src.len()) };
        assert!(!ptr.is_null(), "a well-formed graph must render");
        let svg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        assert!(svg.contains("<svg"), "got: {svg}");
        unsafe { mw_render_graphviz_string_free(ptr) };
        unsafe { mw_render_graphviz_string_free(core::ptr::null_mut()) };
    }

    // A failing render hands back null, which the caller may pass straight to
    // `mw_render_graphviz_string_free`.
    // [spec:mw:sem:ffi.render-shim.lazy-module+1/test]
    #[test]
    fn non_utf8_source_yields_null() {
        let bytes = [0xFFu8, 0xFE];
        assert!(unsafe { mw_render_dot(bytes.as_ptr(), bytes.len()) }.is_null());
        assert!(unsafe { mw_render_dot(core::ptr::null(), 0) }.is_null());
    }
}
