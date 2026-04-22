//! DOT-to-SVG renderer wrapping libgvc / libcgraph via a small C shim.
//!
//! The C shim (`wrapper.c`) installs an `agseterrf` callback so that
//! libgraphviz's diagnostics land in an owned buffer rather than being
//! written to the host process's stderr. A process-global `Mutex`
//! serialises calls — libgraphviz keeps per-context state that is not
//! safe to drive concurrently.

use core::fmt;
use std::ffi::{CString, c_char, c_int};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub enum GraphvizError {
    /// DOT source could not be parsed, laid out, or rendered. The
    /// payload is whatever diagnostic text libgraphviz emitted (may be
    /// empty if the failure happened before any diagnostic was logged).
    RenderFailed(String),
    /// Render produced output that wasn't valid UTF-8. Never observed
    /// for the `svg` format in practice, but surface it rather than
    /// panic.
    NonUtf8Output,
    /// Input string contained an interior NUL byte so we couldn't hand
    /// it to libgraphviz.
    InvalidInput,
}

impl fmt::Display for GraphvizError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RenderFailed(msg) if msg.is_empty() => f.write_str("graphviz render failed"),
            Self::RenderFailed(msg) => write!(f, "graphviz render failed: {}", msg.trim_end()),
            Self::NonUtf8Output => f.write_str("graphviz produced non-UTF-8 SVG"),
            Self::InvalidInput => f.write_str("DOT source contained interior NUL byte"),
        }
    }
}

impl std::error::Error for GraphvizError {}

unsafe extern "C" {
    fn markdawn_gv_render_svg(
        dot: *const c_char,
        out_svg: *mut *mut c_char,
        out_len: *mut usize,
        out_err: *mut *mut c_char,
    ) -> c_int;
    fn markdawn_gv_free_render_data(ptr: *mut c_char);
    fn markdawn_gv_free_error(ptr: *mut c_char);
}

static RENDER_LOCK: Mutex<()> = Mutex::new(());

pub fn render_dot_to_svg(source: &str) -> Result<String, GraphvizError> {
    let dot = CString::new(source).map_err(|_| GraphvizError::InvalidInput)?;

    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut svg_ptr: *mut c_char = core::ptr::null_mut();
    let mut svg_len: usize = 0;
    let mut err_ptr: *mut c_char = core::ptr::null_mut();

    let rc = unsafe {
        markdawn_gv_render_svg(dot.as_ptr(), &mut svg_ptr, &mut svg_len, &mut err_ptr)
    };

    if rc != 0 {
        let msg = take_c_string(err_ptr);
        unsafe { markdawn_gv_free_error(err_ptr) };
        return Err(GraphvizError::RenderFailed(msg));
    }

    if svg_ptr.is_null() {
        return Err(GraphvizError::RenderFailed(String::new()));
    }

    let slice = unsafe { core::slice::from_raw_parts(svg_ptr as *const u8, svg_len) };
    let result = core::str::from_utf8(slice).map(str::to_owned);
    unsafe { markdawn_gv_free_render_data(svg_ptr) };

    result.map_err(|_| GraphvizError::NonUtf8Output)
}

fn take_c_string(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
    cstr.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_simple_digraph() {
        let svg = render_dot_to_svg("digraph { A -> B }").expect("render ok");
        assert!(
            svg.starts_with("<?xml") || svg.contains("<svg"),
            "got: {svg}"
        );
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn malformed_source_returns_err_with_message() {
        let err = render_dot_to_svg("not valid dot {{{{").unwrap_err();
        match err {
            GraphvizError::RenderFailed(msg) => {
                assert!(!msg.is_empty(), "expected captured diagnostic, got empty");
            }
            other => panic!("expected RenderFailed, got {other:?}"),
        }
    }

    #[test]
    fn interior_nul_rejected() {
        let err = render_dot_to_svg("digraph { \0 }").unwrap_err();
        assert!(matches!(err, GraphvizError::InvalidInput));
    }
}
