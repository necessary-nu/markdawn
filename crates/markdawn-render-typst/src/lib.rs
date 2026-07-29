//! Lazy-loaded typst renderer module for the wasm demo.

use std::ffi::{CString, c_char};

/// # Safety
/// `ptr` must point to `len` readable bytes of UTF-8.
// [spec:mw:sem:ffi.render-shim.lazy-module]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_render_typst(ptr: *const u8, len: usize) -> *mut c_char {
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    let Ok(source) = std::str::from_utf8(bytes) else {
        return core::ptr::null_mut();
    };
    let Ok(svg) = markdawn::typst::render(source) else {
        return core::ptr::null_mut();
    };
    match CString::new(svg) {
        Ok(c) => c.into_raw(),
        Err(_) => core::ptr::null_mut(),
    }
}

/// # Safety
/// `ptr` must be a pointer returned by `mw_render_typst` and not yet freed.
// This module's allocator, not the core module's — the two heaps are distinct.
// [spec:mw:req:ffi.ownership.string-free]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(ptr) });
}
