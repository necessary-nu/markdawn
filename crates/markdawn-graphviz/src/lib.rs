//! DOT-to-SVG renderer wrapping libgvc / libcgraph via a small C shim.
//!
//! The C shim (`wrapper.c`) installs an `agseterrf` callback so that
//! libgraphviz's diagnostics land in an owned buffer rather than being
//! written to the host process's stderr. A process-global `Mutex`
//! serialises calls — libgraphviz keeps per-context state that is not
//! safe to drive concurrently.
//!
//! To keep pathological input from taking down the whole host process,
//! [`render_dot_to_svg`] applies two pre-flight gates before handing
//! bytes to libgraphviz: an input-size cap ([`MAX_DOT_BYTES`]) and a
//! brace-nesting-depth cap ([`MAX_BRACE_DEPTH`]). The render itself
//! runs on a dedicated thread with a generous stack; Rust panics from
//! the glue code are captured via `join`. Note that C-level `abort()`
//! / `SIGSEGV` inside libgraphviz would still terminate the whole
//! process — the gates are the real defence against those.

use core::fmt;
use std::ffi::{CString, c_char, c_int};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

mod scrubber;

pub use scrubber::{
    MAX_ATTRIBUTES, MAX_EDGES, MAX_HTML_DEPTH, MAX_MINLEN, MAX_NODES, MAX_RECORD_DEPTH,
    MAX_SUBGRAPHS,
};

/// Reject DOT source larger than this many bytes before any libgraphviz
/// call. Large enough for realistic diagrams; small enough that the
/// worst adversarial inputs never reach the layout engine.
// adv[impl input.byte-cap]
pub const MAX_DOT_BYTES: usize = 256 * 1024;

/// Reject DOT source whose `{`/`}` nesting depth exceeds this. Deeply
/// nested subgraphs are the main way to trigger stack overflow inside
/// libgraphviz's recursive layout.
// adv[impl input.brace-depth-cap]
pub const MAX_BRACE_DEPTH: usize = 64;

/// Thread stack size for the dedicated render thread. Larger than the
/// Rust default so moderate libgraphviz recursion doesn't overflow,
/// while still bounded.
const RENDER_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Wall-clock timeout for a single render call. Bounds how long a
/// pathological input that passed every pre-flight cap can occupy
/// libgraphviz. On timeout, [`render_dot_to_svg`] returns
/// [`GraphvizError::TimedOut`]; the worker thread cannot be killed
/// safely (libgraphviz is C, non-cooperative) so it continues running
/// until libgraphviz returns. See the `adv[render.timeout]` spec.
pub const RENDER_TIMEOUT_SECS: u64 = 10;

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
    /// Input exceeded [`MAX_DOT_BYTES`]. The numeric payload is the
    /// actual byte count.
    InputTooLarge(usize),
    /// Input exceeded [`MAX_BRACE_DEPTH`] in `{`/`}` nesting. The
    /// numeric payload is the observed peak depth.
    InputTooDeep(usize),
    /// Record-label `{`/`}` nesting inside a quoted string exceeded
    /// [`MAX_RECORD_DEPTH`].
    RecordLabelTooDeep(usize),
    /// HTML-label tag nesting exceeded [`MAX_HTML_DEPTH`].
    HtmlLabelTooDeep(usize),
    /// Heuristic node count exceeded [`MAX_NODES`].
    TooManyNodes(usize),
    /// Heuristic edge count (expanded for `A -> { … }` shorthand)
    /// exceeded [`MAX_EDGES`].
    TooManyEdges(usize),
    /// Attribute-list count exceeded [`MAX_ATTRIBUTES`].
    TooManyAttributes(usize),
    /// Subgraph count exceeded [`MAX_SUBGRAPHS`].
    TooManySubgraphs(usize),
    /// The render thread panicked before producing a result. Payload is
    /// the panic message if it was recoverable as `&str`/`String`, else
    /// empty.
    Panicked(String),
    /// The render thread could not be spawned.
    ThreadSpawn(String),
    /// The render exceeded [`RENDER_TIMEOUT_SECS`] wall-clock. The
    /// worker thread continues running in the background because
    /// libgraphviz cannot be safely killed from outside; subsequent
    /// callers will also time out until the stuck render completes.
    TimedOut,
}

impl fmt::Display for GraphvizError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RenderFailed(msg) if msg.is_empty() => f.write_str("graphviz render failed"),
            Self::RenderFailed(msg) => write!(f, "graphviz render failed: {}", msg.trim_end()),
            Self::NonUtf8Output => f.write_str("graphviz produced non-UTF-8 SVG"),
            Self::InvalidInput => f.write_str("DOT source contained interior NUL byte"),
            Self::InputTooLarge(n) => write!(
                f,
                "DOT source is {n} bytes, exceeds cap of {MAX_DOT_BYTES} bytes"
            ),
            Self::InputTooDeep(d) => write!(
                f,
                "DOT source brace nesting is {d}, exceeds cap of {MAX_BRACE_DEPTH}"
            ),
            Self::RecordLabelTooDeep(d) => write!(
                f,
                "DOT record-label brace nesting is {d}, exceeds cap of {MAX_RECORD_DEPTH}"
            ),
            Self::HtmlLabelTooDeep(d) => write!(
                f,
                "DOT HTML-label tag nesting is {d}, exceeds cap of {MAX_HTML_DEPTH}"
            ),
            Self::TooManyNodes(n) => write!(f, "DOT source has {n} nodes, exceeds cap of {MAX_NODES}"),
            Self::TooManyEdges(n) => write!(f, "DOT source has {n} edges, exceeds cap of {MAX_EDGES}"),
            Self::TooManyAttributes(n) => {
                write!(f, "DOT source has {n} attribute lists, exceeds cap of {MAX_ATTRIBUTES}")
            }
            Self::TooManySubgraphs(n) => {
                write!(f, "DOT source has {n} subgraphs, exceeds cap of {MAX_SUBGRAPHS}")
            }
            Self::Panicked(msg) if msg.is_empty() => f.write_str("graphviz render thread panicked"),
            Self::Panicked(msg) => write!(f, "graphviz render thread panicked: {msg}"),
            Self::ThreadSpawn(msg) => write!(f, "could not spawn graphviz render thread: {msg}"),
            Self::TimedOut => write!(
                f,
                "graphviz render exceeded {RENDER_TIMEOUT_SECS}s wall-clock timeout"
            ),
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

/// Render DOT source to SVG with a custom wall-clock timeout. Exposed
/// for tests and for callers that want a tighter budget than the
/// default [`RENDER_TIMEOUT_SECS`]. Pre-render input caps are applied
/// before the timer starts.
pub fn render_dot_to_svg_with_timeout(
    source: &str,
    timeout: Duration,
) -> Result<String, GraphvizError> {
    render_inner_pipeline(source, timeout)
}

pub fn render_dot_to_svg(source: &str) -> Result<String, GraphvizError> {
    render_inner_pipeline(source, Duration::from_secs(RENDER_TIMEOUT_SECS))
}

fn render_inner_pipeline(source: &str, timeout: Duration) -> Result<String, GraphvizError> {
    if source.len() > MAX_DOT_BYTES {
        return Err(GraphvizError::InputTooLarge(source.len()));
    }
    let depth = max_brace_depth(source);
    if depth > MAX_BRACE_DEPTH {
        return Err(GraphvizError::InputTooDeep(depth));
    }

    let (scrubbed, report) = scrubber::scan_and_scrub(source)?;
    if report.identifiers > MAX_NODES {
        return Err(GraphvizError::TooManyNodes(report.identifiers));
    }
    if report.edges > MAX_EDGES {
        return Err(GraphvizError::TooManyEdges(report.edges));
    }
    if report.attributes > MAX_ATTRIBUTES {
        return Err(GraphvizError::TooManyAttributes(report.attributes));
    }
    if report.subgraphs > MAX_SUBGRAPHS {
        return Err(GraphvizError::TooManySubgraphs(report.subgraphs));
    }

    // adv[impl render.timeout] — spawn the worker, wait on a channel
    // with a wall-clock timeout. On timeout the worker is orphaned; it
    // keeps running until libgraphviz returns. Next caller's worker
    // will block on RENDER_LOCK and will itself time out here.
    // adv[impl render.panic-capture] — std::panic::catch_unwind inside
    // the worker captures Rust panics; they arrive on the channel as
    // Err(panic_payload).
    let owned = scrubbed;
    let (tx, rx) = mpsc::channel::<Result<Result<String, GraphvizError>, Box<dyn std::any::Any + Send>>>();
    std::thread::Builder::new()
        .stack_size(RENDER_STACK_BYTES)
        .name("markdawn-graphviz-render".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                render_inner(&owned)
            }));
            let _ = tx.send(result);
        })
        .map_err(|e| GraphvizError::ThreadSpawn(e.to_string()))?;

    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => result,
        Ok(Err(panic_payload)) => Err(GraphvizError::Panicked(panic_message(&panic_payload))),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(GraphvizError::TimedOut),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // Worker dropped the sender without sending — shouldn't
            // happen because the closure always sends before returning.
            // If it does, treat as a panic with no message.
            Err(GraphvizError::Panicked(String::new()))
        }
    }
}

// adv[impl render.error-callback-capture]
// The C shim (`wrapper.c`) installs `agseterrf` on entry and restores the
// previous callback on exit; diagnostics flow back to us via out_err and
// get wrapped into `GraphvizError::RenderFailed`. Rust-side this is just
// the bridge call.
fn render_inner(source: &str) -> Result<String, GraphvizError> {
    let dot = CString::new(source).map_err(|_| GraphvizError::InvalidInput)?;

    // adv[impl render.serialise]
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

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_owned();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    String::new()
}

/// Peak depth of `{` / `}` nesting, ignoring braces that appear inside
/// DOT double-quoted strings (and the escaped `\"` within them).
/// Line comments (`//…`) and block comments (`/* … */`) are not
/// stripped — brace counts inside them still count toward the cap, so
/// the check is conservative (errs on the side of rejecting more).
fn max_brace_depth(source: &str) -> usize {
    let mut depth: usize = 0;
    let mut max: usize = 0;
    let mut in_string = false;
    let mut escape = false;
    for b in source.bytes() {
        if escape {
            escape = false;
            continue;
        }
        match b {
            b'\\' if in_string => escape = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => {
                depth += 1;
                if depth > max {
                    max = depth;
                }
            }
            b'}' if !in_string => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    max
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

    // adv[verify render.error-callback-capture]
    // If err_sink wasn't installed, this test would have nothing to
    // assert non-empty on (libgraphviz would have written straight to
    // stderr).
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

    #[test]
    fn oversize_input_rejected_before_libgraphviz() {
        let huge = "a".repeat(MAX_DOT_BYTES + 1);
        let err = render_dot_to_svg(&huge).unwrap_err();
        match err {
            GraphvizError::InputTooLarge(n) => assert_eq!(n, MAX_DOT_BYTES + 1),
            other => panic!("expected InputTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn deeply_nested_braces_rejected() {
        let mut src = String::from("digraph ");
        for _ in 0..(MAX_BRACE_DEPTH + 1) {
            src.push_str("{ subgraph ");
        }
        for _ in 0..(MAX_BRACE_DEPTH + 1) {
            src.push_str(" }");
        }
        let err = render_dot_to_svg(&src).unwrap_err();
        match err {
            GraphvizError::InputTooDeep(d) => assert!(d > MAX_BRACE_DEPTH),
            other => panic!("expected InputTooDeep, got {other:?}"),
        }
    }

    #[test]
    fn braces_inside_strings_do_not_count_toward_depth() {
        assert_eq!(max_brace_depth(r#"digraph { a [label="{{{{{{"] }"#), 1);
    }

    #[test]
    fn escaped_quote_inside_string_preserves_string_state() {
        // The \" is escaped, so the string doesn't close until the
        // final literal quote. The { inside stays inside the string.
        assert_eq!(max_brace_depth(r#"a "b\"{" c"#), 0);
    }
}
