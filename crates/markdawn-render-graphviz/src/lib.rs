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
/// `mw_string_free`. Returns null on render failure (the caller
/// should leave the original code block in place).
///
/// # Safety
/// `ptr` must point to `len` readable bytes of UTF-8.
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
/// `ptr` must be a pointer returned by `mw_render_dot` and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(ptr) });
}
