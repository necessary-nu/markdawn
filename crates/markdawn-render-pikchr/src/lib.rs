//! Lazy-loaded pikchr renderer module for the wasm demo.

use std::ffi::{CString, c_char};

/// # Safety
/// `ptr` must point to `len` readable bytes of UTF-8.
// [spec:mw:sem:ffi.render-shim.lazy-module+1]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_render_pikchr(ptr: *const u8, len: usize) -> *mut c_char {
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    let Ok(source) = std::str::from_utf8(bytes) else {
        return core::ptr::null_mut();
    };
    let Ok(svg) = markdawn::pikchr_render::render(source) else {
        return core::ptr::null_mut();
    };
    match CString::new(svg) {
        Ok(c) => c.into_raw(),
        Err(_) => core::ptr::null_mut(),
    }
}

/// # Safety
/// `ptr` must be a pointer returned by *this* module's `mw_render_pikchr` and
/// not yet freed. This is this module's allocator over this module's linear
/// memory; a pointer minted by the core module or by another
/// `markdawn-render-*` module is an offset into a different heap, and nothing
/// here can tell the two apart — null is the only argument this function can
/// reject. The engine-qualified name is the guard: no other module exports it,
/// so naming it against the wrong module fails to resolve rather than freeing
/// into the wrong heap.
// [spec:mw:req:ffi.ownership.string-free+2]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_render_pikchr_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(ptr) });
}
