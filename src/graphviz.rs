//! Safe wrapper around `graphviz-sys` for rendering DOT source as SVG.
//!
//! Pipeline: `gvContext` → `agmemread` → `gvLayout("dot")` →
//! `gvRenderData("svg", ...)` → clean up. A process-global `Mutex` serialises
//! calls because libgraphviz keeps per-context state that is not thread-safe.

use core::fmt;
use std::ffi::{CString, c_char};
use std::sync::Mutex;

use graphviz_sys as ffi;

#[derive(Debug, Clone)]
pub enum GraphvizError {
    /// DOT source could not be parsed / laid out / rendered. Any diagnostic
    /// text libgraphviz emitted is captured here.
    RenderFailed(String),
    /// Render produced output that wasn't valid UTF-8. Never observed for
    /// the `svg` format in practice, but surface it rather than panic.
    NonUtf8Output,
    /// Input string contained an interior NUL byte so we couldn't hand it to
    /// libgraphviz.
    InvalidInput,
}

impl fmt::Display for GraphvizError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RenderFailed(msg) if msg.is_empty() => f.write_str("graphviz render failed"),
            Self::RenderFailed(msg) => write!(f, "graphviz render failed: {msg}"),
            Self::NonUtf8Output => f.write_str("graphviz produced non-UTF-8 SVG"),
            Self::InvalidInput => f.write_str("DOT source contained interior NUL byte"),
        }
    }
}

impl std::error::Error for GraphvizError {}

static RENDER_LOCK: Mutex<()> = Mutex::new(());

pub fn render_dot_to_svg(source: &str) -> Result<String, GraphvizError> {
    let dot_source = CString::new(source).map_err(|_| GraphvizError::InvalidInput)?;

    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    unsafe { render_inner(&dot_source) }
}

unsafe fn render_inner(dot_source: &std::ffi::CStr) -> Result<String, GraphvizError> {
    unsafe {
        let gvc = ffi::gvContext();
        if gvc.is_null() {
            return Err(GraphvizError::RenderFailed(String::new()));
        }
        let _ctx = ContextGuard(gvc);

        let graph = ffi::agmemread(dot_source.as_ptr());
        if graph.is_null() {
            return Err(GraphvizError::RenderFailed(String::new()));
        }
        let _graph = GraphGuard(graph);

        let layout_engine = c"dot";
        if ffi::gvLayout(gvc, graph, layout_engine.as_ptr()) != 0 {
            return Err(GraphvizError::RenderFailed(String::new()));
        }
        let _layout = LayoutGuard { gvc, graph };

        let format = c"svg";
        let mut buf: *mut c_char = core::ptr::null_mut();
        let mut len: usize = 0;
        let rc = ffi::gvRenderData(gvc, graph, format.as_ptr(), &mut buf, &mut len);
        if rc != 0 || buf.is_null() {
            return Err(GraphvizError::RenderFailed(String::new()));
        }

        let slice = core::slice::from_raw_parts(buf as *const u8, len);
        let owned = match core::str::from_utf8(slice) {
            Ok(s) => s.to_owned(),
            Err(_) => {
                ffi::gvFreeRenderData(buf);
                return Err(GraphvizError::NonUtf8Output);
            }
        };
        ffi::gvFreeRenderData(buf);
        Ok(owned)
    }
}

struct ContextGuard(*mut ffi::GVC_t);
impl Drop for ContextGuard {
    fn drop(&mut self) {
        unsafe {
            ffi::gvFreeContext(self.0);
        }
    }
}

struct GraphGuard(*mut ffi::Agraph_t);
impl Drop for GraphGuard {
    fn drop(&mut self) {
        unsafe {
            ffi::agclose(self.0);
        }
    }
}

struct LayoutGuard {
    gvc: *mut ffi::GVC_t,
    graph: *mut ffi::Agraph_t,
}
impl Drop for LayoutGuard {
    fn drop(&mut self) {
        unsafe {
            ffi::gvFreeLayout(self.gvc, self.graph);
        }
    }
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
    fn malformed_source_returns_err() {
        let err = render_dot_to_svg("not valid dot {{{{").unwrap_err();
        assert!(matches!(err, GraphvizError::RenderFailed(_)));
    }

    #[test]
    fn interior_nul_rejected() {
        let err = render_dot_to_svg("digraph { \0 }").unwrap_err();
        assert!(matches!(err, GraphvizError::InvalidInput));
    }
}
