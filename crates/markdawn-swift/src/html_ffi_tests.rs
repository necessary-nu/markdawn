//! Ownership and failure-conversion contracts of the C ABI, driven through
//! the exported `extern "C"` entry points exactly as a Swift/C host would.

use super::*;
use std::ffi::CStr;

/// Finish, render and release a handle, returning the HTML snapshot.
fn finish_and_render(state: *mut State) -> String {
    assert_eq!(unsafe { mw_state_finish(state) }, 0);
    let ptr = unsafe { mw_state_render(state) };
    assert!(!ptr.is_null());
    let html = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    unsafe { mw_string_free(ptr) };
    unsafe { mw_state_free(state) };
    html
}

// [spec:mw:req:ffi.ownership.borrowed-input/test]
#[test]
fn a_pushed_chunk_is_copied_before_the_call_returns() {
    let state = mw_state_new();

    let mut buffer = b"# Heading\n\nbody text\n".to_vec();
    assert_eq!(
        unsafe { mw_state_push_chunk(state, buffer.as_ptr(), buffer.len()) },
        0
    );
    // The host keeps ownership and may reuse or free the buffer the moment
    // the call returns. Scribble over it, then release it entirely.
    buffer.fill(0xAA);
    drop(buffer);

    let html = finish_and_render(state);
    assert!(html.contains("<h1"), "got: {html}");
    assert!(html.contains("body text"), "got: {html}");
    assert!(!html.contains('\u{FFFD}'), "scrubbed bytes leaked: {html}");
}

// [spec:mw:req:ffi.ownership.borrowed-input/test]
#[test]
fn a_borrowed_chunk_is_not_retained_across_pushes() {
    let state = mw_state_new();

    // Three separate short-lived buffers; if any were retained rather than
    // copied, the later pushes would read freed memory.
    for line in ["alpha\n", "\n", "beta"] {
        let buffer = line.as_bytes().to_vec();
        assert_eq!(
            unsafe { mw_state_push_chunk(state, buffer.as_ptr(), buffer.len()) },
            0
        );
        drop(buffer);
    }

    // A single byte borrowed from a temporary, likewise.
    {
        let bytes = "!".to_owned().into_bytes();
        assert_eq!(unsafe { mw_state_push_byte(state, bytes[0]) }, 0);
    }

    let html = finish_and_render(state);
    assert!(html.contains("alpha"), "got: {html}");
    assert!(html.contains("beta!"), "got: {html}");
}

// [spec:mw:req:ffi.ownership.handle-release/test]
#[test]
fn every_handle_is_released_through_the_destructor_paired_with_its_constructor() {
    // Rust allocates each handle, and each is a distinct live allocation.
    let handles: Vec<*mut State> = (0..16).map(|_| mw_state_new()).collect();
    assert!(handles.iter().all(|p| !p.is_null()));
    let mut addresses: Vec<usize> = handles.iter().map(|p| *p as usize).collect();
    addresses.sort_unstable();
    addresses.dedup();
    assert_eq!(addresses.len(), handles.len(), "handles must not alias");

    // `mw_state_new` pairs with `mw_state_free` — never the C library's
    // `free`, which cannot honour Rust's layout metadata.
    for handle in handles {
        unsafe { mw_state_free(handle) };
    }

    // A fresh handle still works after the previous ones were released, so
    // release did not disturb shared state.
    let state = mw_state_new();
    let src = b"after\n";
    assert_eq!(
        unsafe { mw_state_push_chunk(state, src.as_ptr(), src.len()) },
        0
    );
    assert!(finish_and_render(state).contains("after"));
}

// [spec:mw:req:ffi.ownership.handle-release/test]
#[test]
fn destructors_accept_a_null_pointer_and_do_nothing() {
    // A host may call them unconditionally on a nullable field.
    unsafe { mw_state_free(core::ptr::null_mut()) };
    unsafe { mw_state_free(core::ptr::null_mut()) };
    unsafe { mw_string_free(core::ptr::null_mut()) };
    unsafe { mw_string_free(core::ptr::null_mut()) };

    // And the accessor that mints a string likewise reports absence rather
    // than handing back something that must not be released.
    assert!(unsafe { mw_state_render(core::ptr::null()) }.is_null());
}

// [spec:mw:req:ffi.safety.no-unwind/test]
#[test]
fn an_expected_failure_becomes_a_status_code_rather_than_an_unwind() {
    let state = mw_state_new();
    assert_eq!(unsafe { mw_state_finish(state) }, 0);

    // The parser rejects work after `finish`. That error has to cross the
    // boundary as a `u32`, since a panic would abort under the release
    // profile's `panic = "abort"` instead of unwinding into the host.
    assert_eq!(unsafe { mw_state_finish(state) }, 1);
    let src = b"more\n";
    assert_eq!(
        unsafe { mw_state_push_chunk(state, src.as_ptr(), src.len()) },
        1
    );
    assert_eq!(unsafe { mw_state_push_byte(state, b'x') }, 1);

    // Every status code is 0 for success and non-zero for failure — there
    // is no out-of-band channel to consult.
    assert_eq!(unsafe { mw_state_finish(core::ptr::null_mut()) }, 1);

    unsafe { mw_state_free(state) };
}

// [spec:mw:req:ffi.safety.no-unwind/test]
#[test]
fn where_the_return_type_is_a_pointer_failure_is_a_null_pointer() {
    // No `catch_unwind` wrapper exists, so the fallible paths convert to
    // null before returning.
    assert!(unsafe { mw_state_render(core::ptr::null()) }.is_null());

    // A successful render is non-null and NUL-terminated, so a host can
    // tell the two apart without an error channel.
    let state = mw_state_new();
    let src = b"text\n";
    assert_eq!(
        unsafe { mw_state_push_chunk(state, src.as_ptr(), src.len()) },
        0
    );
    assert_eq!(unsafe { mw_state_finish(state) }, 0);
    let ptr = unsafe { mw_state_render(state) };
    assert!(!ptr.is_null());
    assert!(
        unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .expect("valid UTF-8")
            .contains("text")
    );
    unsafe { mw_string_free(ptr) };
    unsafe { mw_state_free(state) };
}
