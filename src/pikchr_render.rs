//! Hardened wrapper around `pikru::pikchr_with_options`.
//!
//! Mirrors the defensive layer around libgraphviz in
//! [`markdawn_graphviz`]: input byte cap, wall-clock render timeout,
//! panic capture, and a defensive process-global `Mutex`. libpikchr is
//! reportedly reentrant so the Mutex is belt-and-suspenders rather
//! than strictly required, but its cost is negligible and it matches
//! the shape of the graphviz wrapper.

use core::fmt;
use std::sync::{Mutex, mpsc};
use std::time::Duration;

/// Reject pikchr source larger than this many bytes before calling
/// libpikchr. Large enough for realistic diagrams; small enough that
/// OOM / CPU-bomb attempts fail fast.
// adv[impl pikchr.input.byte-cap]
pub const MAX_PIKCHR_BYTES: usize = 256 * 1024;

/// Wall-clock budget for a single pikchr render. Mirrors graphviz.
// adv[impl pikchr.render.timeout]
pub const PIKCHR_RENDER_TIMEOUT_SECS: u64 = 10;

/// Thread stack size for the pikchr render worker.
const RENDER_STACK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum PikchrError {
    /// Source exceeded [`MAX_PIKCHR_BYTES`]. Payload is the observed
    /// byte count.
    InputTooLarge(usize),
    /// libpikchr rejected the source. Payload is the human-readable
    /// error report from `pikru` (parse error message, line/column,
    /// and source snippet).
    Failed(String),
    /// The render worker thread panicked. Payload is the recovered
    /// message if any.
    Panicked(String),
    /// The render exceeded the wall-clock timeout. The worker thread
    /// continues running until libpikchr returns.
    TimedOut,
    /// The render worker thread could not be spawned.
    ThreadSpawn(String),
}

impl fmt::Display for PikchrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge(n) => write!(
                f,
                "pikchr source is {n} bytes, exceeds cap of {MAX_PIKCHR_BYTES} bytes"
            ),
            Self::Failed(msg) if msg.is_empty() => f.write_str("pikchr render failed"),
            Self::Failed(msg) => write!(f, "pikchr render failed:\n{msg}"),
            Self::Panicked(msg) if msg.is_empty() => f.write_str("pikchr render thread panicked"),
            Self::Panicked(msg) => write!(f, "pikchr render thread panicked: {msg}"),
            Self::TimedOut => write!(
                f,
                "pikchr render exceeded {PIKCHR_RENDER_TIMEOUT_SECS}s wall-clock timeout"
            ),
            Self::ThreadSpawn(msg) => write!(f, "could not spawn pikchr render thread: {msg}"),
        }
    }
}

impl std::error::Error for PikchrError {}

// adv[impl pikchr.render.serialise]
static RENDER_LOCK: Mutex<()> = Mutex::new(());

/// Render a pikchr source as SVG with the default timeout.
pub fn render(source: &str) -> Result<String, PikchrError> {
    render_with_timeout(source, Duration::from_secs(PIKCHR_RENDER_TIMEOUT_SECS))
}

/// Render a pikchr source as SVG with a caller-supplied wall-clock
/// budget. Pre-flight input cap is applied before the timer starts.
pub fn render_with_timeout(source: &str, timeout: Duration) -> Result<String, PikchrError> {
    if source.len() > MAX_PIKCHR_BYTES {
        return Err(PikchrError::InputTooLarge(source.len()));
    }

    let owned = source.to_owned();
    let (tx, rx) =
        mpsc::channel::<Result<Result<String, PikchrError>, Box<dyn std::any::Any + Send>>>();

    // adv[impl pikchr.render.timeout]
    // adv[impl pikchr.render.panic-capture]
    std::thread::Builder::new()
        .stack_size(RENDER_STACK_BYTES)
        .name("markdawn-pikchr-render".to_owned())
        .spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_inner(&owned)));
            let _ = tx.send(result);
        })
        .map_err(|e| PikchrError::ThreadSpawn(e.to_string()))?;

    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => result,
        Ok(Err(panic_payload)) => Err(PikchrError::Panicked(panic_message(&panic_payload))),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(PikchrError::TimedOut),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(PikchrError::Panicked(String::new())),
    }
}

fn render_inner(source: &str) -> Result<String, PikchrError> {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let opts = pikru::RenderOptions {
        css_variables: true,
    };
    pikru::pikchr_with_options(source, &opts).map_err(PikchrError::Failed)
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

#[cfg(test)]
mod tests {
    use super::*;

    // adv[verify pikchr.input.byte-cap]
    #[test]
    fn oversize_input_rejected() {
        let huge = "a".repeat(MAX_PIKCHR_BYTES + 1);
        let err = render(&huge).unwrap_err();
        match err {
            PikchrError::InputTooLarge(n) => assert_eq!(n, MAX_PIKCHR_BYTES + 1),
            other => panic!("expected InputTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn benign_source_renders() {
        let svg = render(r#"box "Hello""#).expect("render ok");
        assert!(svg.contains("<svg"), "got: {svg}");
    }

    // adv[verify pikchr.render.timeout]
    #[test]
    fn timeout_fires_with_zero_budget() {
        let err = render_with_timeout(r#"box "Hi""#, Duration::from_millis(0)).unwrap_err();
        assert!(matches!(err, PikchrError::TimedOut), "got: {err:?}");
    }

    // adv[verify pikchr.render.panic-capture]
    #[test]
    fn panic_variant_round_trips_through_display() {
        let err = PikchrError::Panicked("boom".to_owned());
        let s = format!("{err}");
        assert!(s.to_lowercase().contains("panic"), "got: {s}");
        assert!(s.contains("boom"), "got: {s}");
    }

    // adv[verify pikchr.render.serialise]
    #[test]
    fn concurrent_renders_serialize() {
        let handles: Vec<_> = (0..8)
            .map(|i| std::thread::spawn(move || render(&format!(r#"box "box-{i}""#))))
            .collect();
        for h in handles {
            let svg = h.join().expect("no panic").expect("render ok");
            assert!(svg.contains("<svg"), "got: {svg}");
        }
    }
}
