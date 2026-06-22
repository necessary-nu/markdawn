//! Lazy-loaded pikchr renderer module for the wasm demo.

use std::ffi::{CString, c_char};

/// # Safety
/// `ptr` must point to `len` readable bytes of UTF-8.
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
/// `ptr` must be a pointer returned by `mw_render_pikchr` and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(ptr) });
}
