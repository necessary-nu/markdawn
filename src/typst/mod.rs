//! Hardened wrapper around the `typst` compiler for rendering single
//! fenced `typst` code blocks to inline SVG.
//!
//! The defensive shape mirrors [`crate::pikchr_render`]: input byte
//! cap, wall-clock render timeout, panic capture, and a process-wide
//! `Mutex`. typst is reentrant (each compile uses its own [`World`]
//! instance) but the comemo cache is process-wide, so the lock keeps
//! cache contention deterministic and matches the pikchr pattern.

mod inline_uses;
mod world;

use core::fmt;
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use typst::WorldExt;
use typst::layout::Abs;
use typst_layout::PagedDocument;

use self::world::RenderWorld;

/// Reject typst source larger than this many bytes before invoking the
/// compiler for **SVG** rendering. Sized for diagram-fence usage —
/// large enough for realistic diagrams, small enough that pathological
/// inputs fail fast. Mirrors pikchr.
pub const MAX_TYPST_BYTES: usize = 256 * 1024;

/// Wall-clock budget for a single typst SVG render. Mirrors pikchr.
pub const TYPST_RENDER_TIMEOUT_SECS: u64 = 10;

/// Reject typst source larger than this many bytes before invoking the
/// compiler for **PDF** rendering. Sized for whole-document usage
/// (ebook chapters, articles) which are inherently larger than a
/// per-fence diagram.
pub const MAX_TYPST_PDF_BYTES: usize = 8 * 1024 * 1024;

/// Wall-clock budget for a single typst PDF render. Higher than the
/// SVG budget because PDF compiles a whole document, not one diagram.
pub const TYPST_PDF_RENDER_TIMEOUT_SECS: u64 = 60;

/// Padding around the merged-page SVG output (in points).
const SVG_PADDING_PT: f64 = 4.0;

/// Stack size for the render worker thread. Typst recurses deeply
/// during layout for non-trivial documents.
const RENDER_STACK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum TypstRenderError {
    /// Source exceeded [`MAX_TYPST_BYTES`].
    InputTooLarge(usize),
    /// The typst compiler reported errors. Payload is a human-readable
    /// summary of the diagnostic vector.
    Failed(String),
    /// The render worker thread panicked.
    Panicked(String),
    /// The render exceeded the wall-clock timeout.
    TimedOut,
    /// The render worker thread could not be spawned.
    ThreadSpawn(String),
}

impl fmt::Display for TypstRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge(n) => {
                write!(f, "typst source is {n} bytes, exceeds the configured cap")
            }
            Self::Failed(msg) if msg.is_empty() => f.write_str("typst render failed"),
            Self::Failed(msg) => write!(f, "typst render failed:\n{msg}"),
            Self::Panicked(msg) if msg.is_empty() => f.write_str("typst render thread panicked"),
            Self::Panicked(msg) => write!(f, "typst render thread panicked: {msg}"),
            Self::TimedOut => f.write_str("typst render exceeded the wall-clock timeout"),
            Self::ThreadSpawn(msg) => write!(f, "could not spawn typst render thread: {msg}"),
        }
    }
}

impl std::error::Error for TypstRenderError {}

// [spec:adv:req:typst.render.serialise]
static RENDER_LOCK: Mutex<()> = Mutex::new(());

/// Render a typst source as SVG with the default timeout.
pub fn render(source: &str) -> Result<String, TypstRenderError> {
    render_with_timeout(source, Duration::from_secs(TYPST_RENDER_TIMEOUT_SECS))
}

/// Render a typst source as SVG with a caller-supplied wall-clock
/// budget. Pre-flight input cap is applied before the timer starts.
// [spec:adv:req:typst.input.byte-cap]
pub fn render_with_timeout(source: &str, timeout: Duration) -> Result<String, TypstRenderError> {
    if source.len() > MAX_TYPST_BYTES {
        return Err(TypstRenderError::InputTooLarge(source.len()));
    }

    let owned = wrap_with_preamble(source);

    // wasm has no real threads — `std::thread::spawn` errors on
    // wasm32-unknown-emscripten without `-pthread` and is a no-op on
    // wasm32-unknown-unknown. The wasm host is single-threaded and
    // already sandboxed, so just call render_inner directly. The
    // pre-flight byte cap remains the line of defence.
    #[cfg(target_family = "wasm")]
    {
        let _ = timeout;
        return render_inner(owned);
    }

    #[cfg(not(target_family = "wasm"))]
    {
        let (tx, rx) = mpsc::channel::<
            Result<Result<String, TypstRenderError>, Box<dyn std::any::Any + Send>>,
        >();

        // [spec:adv:req:typst.render.timeout]
        // [spec:adv:req:typst.render.panic-capture]
        std::thread::Builder::new()
            .stack_size(RENDER_STACK_BYTES)
            .name("markdawn-typst".to_owned())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_inner(owned)));
                let _ = tx.send(result);
            })
            .map_err(|e| TypstRenderError::ThreadSpawn(e.to_string()))?;

        match rx.recv_timeout(timeout) {
            Ok(Ok(result)) => result,
            Ok(Err(panic_payload)) => {
                Err(TypstRenderError::Panicked(panic_message(&panic_payload)))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(TypstRenderError::TimedOut),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(TypstRenderError::Panicked(String::new()))
            }
        }
    }
}

/// Compile a typst source to a PDF byte stream with the default
/// timeout. Unlike [`render`] (SVG), this path does **not** wrap the
/// input with a `#set page(width: auto, ...)` preamble — PDF callers
/// produce whole documents and supply their own `#set page(...)`,
/// page numbers, TOC, etc.
///
/// Designed to replace shelling out to the external `typst compile`
/// binary in markdown-to-PDF pipelines.
pub fn render_pdf(source: &str) -> Result<Vec<u8>, TypstRenderError> {
    render_pdf_with_timeout(source, Duration::from_secs(TYPST_PDF_RENDER_TIMEOUT_SECS))
}

/// Render a typst source as PDF with a caller-supplied wall-clock
/// budget. Pre-flight input cap ([`MAX_TYPST_PDF_BYTES`]) is applied
/// before the timer starts.
// [spec:adv:req:typst.input.byte-cap]
pub fn render_pdf_with_timeout(
    source: &str,
    timeout: Duration,
) -> Result<Vec<u8>, TypstRenderError> {
    if source.len() > MAX_TYPST_PDF_BYTES {
        return Err(TypstRenderError::InputTooLarge(source.len()));
    }

    let owned = source.to_owned();
    let (tx, rx) =
        mpsc::channel::<Result<Result<Vec<u8>, TypstRenderError>, Box<dyn std::any::Any + Send>>>();

    // [spec:adv:req:typst.render.timeout]
    // [spec:adv:req:typst.render.panic-capture]
    std::thread::Builder::new()
        .stack_size(RENDER_STACK_BYTES)
        .name("markdawn-typst-pdf".to_owned())
        .spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_pdf_inner(owned)));
            let _ = tx.send(result);
        })
        .map_err(|e| TypstRenderError::ThreadSpawn(e.to_string()))?;

    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => result,
        Ok(Err(panic_payload)) => Err(TypstRenderError::Panicked(panic_message(&panic_payload))),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(TypstRenderError::TimedOut),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(TypstRenderError::Panicked(String::new())),
    }
}

/// Build a `RenderWorld` from `source` and run the typst compiler to
/// produce a `PagedDocument`. The world is returned alongside the
/// document so the caller's export step can format diagnostics that
/// reference source spans.
///
/// The caller must hold [`RENDER_LOCK`] for the duration of the
/// compile, since typst's `comemo` cache is process-wide.
fn compile_to_paged(source: String) -> Result<(RenderWorld, PagedDocument), TypstRenderError> {
    let world = RenderWorld::new(source);
    let warned = typst::compile::<PagedDocument>(&world);
    let document = warned
        .output
        .map_err(|diags| TypstRenderError::Failed(format_diagnostics(&diags, &world)))?;
    Ok((world, document))
}

// [spec:mw:req:typst.preamble.svg-auto-page]
// [spec:mw:req:typst.svg.inline-uses]
fn render_inner(source: String) -> Result<String, TypstRenderError> {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_world, document) = compile_to_paged(source)?;
    let svg = typst_svg::svg_merged(
        &document,
        &typst_svg::SvgOptions::default(),
        Abs::pt(SVG_PADDING_PT),
    );
    // typst-svg emits every glyph as <symbol>+<use xlink:href>; inline
    // those into plain <g>+<path> before sanitisation, since the W3C
    // safe baseline strips <use> wholesale.
    Ok(inline_uses::inline_uses(&svg))
}

fn render_pdf_inner(source: String) -> Result<Vec<u8>, TypstRenderError> {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (world, document) = compile_to_paged(source)?;
    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|diags| TypstRenderError::Failed(format_diagnostics(&diags, &world)))
}

/// Wrap user source in a tiny preamble so the page sizes to its content
/// instead of printing on a blank A4. Hard-coded because there is no
/// per-block configuration surface yet.
// [spec:mw:req:typst.preamble.svg-auto-page]
fn wrap_with_preamble(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 96);
    out.push_str(
        "#set page(width: auto, height: auto, margin: 0.5em, fill: none)\n\
         #set text(size: 11pt)\n",
    );
    out.push_str(source);
    out
}

fn format_diagnostics<W: typst::World>(
    diags: &ecow::EcoVec<typst::diag::SourceDiagnostic>,
    world: &W,
) -> String {
    let mut s = String::new();
    for (idx, diag) in diags.iter().enumerate() {
        if idx > 0 {
            s.push('\n');
        }
        s.push_str(&diag.message);
        if let Some(id) = diag.span.id() {
            if let Ok(source) = world.source(id) {
                if let Some(range) = world.range(diag.span) {
                    if let Some((line, col)) = source.lines().byte_to_line_column(range.start) {
                        s.push_str(&format!(" (line {}, col {})", line + 1, col + 1));
                    }
                }
            }
        }
        for hint in &diag.hints {
            s.push_str("\n  hint: ");
            s.push_str(&hint.v);
        }
    }
    s
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

    // [spec:adv:req:typst.input.byte-cap/test]
    #[test]
    fn oversize_input_rejected() {
        let huge = "a".repeat(MAX_TYPST_BYTES + 1);
        let err = render(&huge).unwrap_err();
        match err {
            TypstRenderError::InputTooLarge(n) => assert_eq!(n, MAX_TYPST_BYTES + 1),
            other => panic!("expected InputTooLarge, got {other:?}"),
        }
    }

    // [spec:mw:def:typst.world.hermetic/test]
    #[test]
    fn benign_source_renders() {
        let svg = render("= Hello\n\nworld").expect("render ok");
        assert!(svg.contains("<svg"), "got: {svg}");
    }

    // [spec:mw:req:typst.packages.offline-resolution/test]
    #[test]
    fn syntree_renders() {
        let src = r#"
#import "@preview/syntree:0.2.1": syntree
#syntree("[S [NP cat] [VP sat]]")
"#;
        let svg = render(src).expect("syntree render ok");
        assert!(svg.contains("<svg"), "expected svg root, got: {svg}");
        assert!(
            svg.contains("<path"),
            "expected at least one <path> from syntree, got: {svg}"
        );
    }

    // [spec:mw:req:typst.packages.offline-resolution/test]
    #[test]
    fn cetz_renders() {
        let src = r#"
#import "@preview/cetz:0.5.0"
#cetz.canvas({
  import cetz.draw: *
  circle((0, 0), radius: 1)
})
"#;
        let svg = render(src).expect("cetz render ok");
        assert!(svg.contains("<svg"), "got: {svg}");
        assert!(
            svg.contains("<path"),
            "expected path output, got len {}",
            svg.len()
        );
    }

    // [spec:adv:req:typst.render.timeout/test]
    #[test]
    fn timeout_fires_with_zero_budget() {
        let err = render_with_timeout("= Hi", Duration::from_millis(0)).unwrap_err();
        assert!(matches!(err, TypstRenderError::TimedOut), "got: {err:?}");
    }

    // [spec:adv:req:typst.render.panic-capture/test]
    #[test]
    fn panic_variant_round_trips_through_display() {
        let err = TypstRenderError::Panicked("boom".to_owned());
        let s = format!("{err}");
        assert!(s.to_lowercase().contains("panic"), "got: {s}");
        assert!(s.contains("boom"), "got: {s}");
    }

    #[test]
    fn malformed_source_returns_failed() {
        // Unbalanced bracket — typst will reject.
        let err = render("#let x = (").unwrap_err();
        assert!(matches!(err, TypstRenderError::Failed(_)), "got: {err:?}");
    }

    // [spec:mw:req:typst.svg.inline-uses/test]
    #[test]
    fn rendered_svg_has_no_use_or_symbol() {
        // After inline_uses runs, the output must be entirely
        // <g>+<path> — both `<use>` and `<symbol>` are stripped by the
        // SVG sanitiser baseline, so leaving them in would erase the
        // text layer downstream.
        let src = r#"#import "@preview/syntree:0.2.1": syntree
#syntree("[S [NP cat]]")"#;
        let svg = render(src).unwrap();
        assert!(!svg.contains("<symbol"), "unexpected <symbol> in output");
        assert!(!svg.contains("<use "), "unexpected <use> in output");
        assert!(!svg.contains("<use/"), "unexpected <use/> in output");
        assert!(
            svg.contains("<path"),
            "expected <path> elements; got: {}",
            &svg[..svg.len().min(200)]
        );
    }

    #[test]
    fn render_pdf_emits_pdf_header() {
        let pdf = render_pdf("= Hello\n\nbody").expect("pdf render ok");
        assert!(
            pdf.starts_with(b"%PDF-"),
            "expected %PDF- magic, got first 16 bytes: {:?}",
            &pdf[..pdf.len().min(16)]
        );
    }

    // [spec:adv:req:typst.input.byte-cap/test]
    #[test]
    fn render_pdf_oversize_input_rejected() {
        let huge = "a".repeat(MAX_TYPST_PDF_BYTES + 1);
        let err = render_pdf(&huge).unwrap_err();
        match err {
            TypstRenderError::InputTooLarge(n) => assert_eq!(n, MAX_TYPST_PDF_BYTES + 1),
            other => panic!("expected InputTooLarge, got {other:?}"),
        }
    }

    // [spec:adv:req:typst.render.timeout/test]
    #[test]
    fn render_pdf_timeout_fires_with_zero_budget() {
        let err = render_pdf_with_timeout("= Hi", Duration::from_millis(0)).unwrap_err();
        assert!(matches!(err, TypstRenderError::TimedOut), "got: {err:?}");
    }

    #[test]
    fn render_pdf_malformed_source_returns_failed() {
        let err = render_pdf("#let x = (").unwrap_err();
        assert!(matches!(err, TypstRenderError::Failed(_)), "got: {err:?}");
    }

    // [spec:mw:req:typst.packages.offline-resolution/test]
    #[test]
    fn render_pdf_uses_bundled_packages() {
        // Proof point for ebook authoring: bundled `@preview/syntree`
        // is reachable from the PDF path too, not just the SVG path.
        let src = r#"#import "@preview/syntree:0.2.1": syntree
#syntree("[S [NP cat] [VP sat]]")"#;
        let pdf = render_pdf(src).expect("syntree pdf ok");
        assert!(pdf.starts_with(b"%PDF-"), "missing PDF header");
        assert!(
            pdf.len() > 4 * 1024,
            "expected non-trivial PDF body, got {} bytes",
            pdf.len()
        );
    }

    // [spec:adv:req:typst.render.serialise/test]
    #[test]
    fn concurrent_renders_serialize() {
        let handles: Vec<_> = (0..4)
            .map(|i| std::thread::spawn(move || render(&format!("= heading-{i}\n\nbody"))))
            .collect();
        for h in handles {
            let svg = h.join().expect("no panic").expect("render ok");
            assert!(svg.contains("<svg"), "got: {svg}");
        }
    }
    // [spec:mw:req:typst.preamble.svg-auto-page/test]
    #[test]
    fn the_preamble_is_exactly_two_lines_of_auto_page_and_eleven_point_text() {
        assert_eq!(
            wrap_with_preamble("body"),
            "#set page(width: auto, height: auto, margin: 0.5em, fill: none)\n\
             #set text(size: 11pt)\n\
             body"
        );
        assert_eq!(wrap_with_preamble("body").lines().count(), 3);
    }

    // [spec:mw:req:typst.preamble.svg-auto-page/test]
    #[test]
    fn the_svg_path_prepends_the_preamble_and_the_pdf_path_does_not() {
        // The preamble becomes part of the compiled source, so an error on
        // the caller's first line is reported two lines further down on the
        // SVG path. The PDF path reports it unshifted, which is how we know
        // it compiled the source as written.
        let svg = render("#nope()").unwrap_err();
        let pdf = render_pdf("#nope()").unwrap_err();
        match (&svg, &pdf) {
            (TypstRenderError::Failed(svg), TypstRenderError::Failed(pdf)) => {
                assert!(svg.contains("(line 3, col 2)"), "svg diagnostic: {svg}");
                assert!(pdf.contains("(line 1, col 2)"), "pdf diagnostic: {pdf}");
            }
            other => panic!("expected two Failed diagnostics, got {other:?}"),
        }
    }

    // [spec:mw:req:typst.preamble.svg-auto-page/test]
    #[test]
    fn the_svg_page_sizes_to_its_content_instead_of_a_blank_a4_sheet() {
        let svg = render("= T\n\ncontent").unwrap();
        let width = svg_root_length(&svg, "width");
        let height = svg_root_length(&svg, "height");
        // A4 is 595.28 x 841.89pt; a one-line fence must be nowhere near it.
        assert!(width < 200.0, "width {width}pt is not content-sized");
        assert!(height < 100.0, "height {height}pt is not content-sized");
        assert!(width > 0.0 && height > 0.0);
    }

    // [spec:mw:req:typst.preamble.svg-auto-page/test]
    #[test]
    fn the_svg_page_has_no_opaque_background() {
        // `fill: none` means typst emits no page-background path at all.
        let transparent = render("content").unwrap();
        assert!(
            !transparent.contains("fill=\"#ffffff\""),
            "opaque background leaked into the figure: {transparent}"
        );
        // A caller who overrides the fill gets the background back, which is
        // what shows the preamble's `fill: none` is doing the work.
        let opaque = render("#set page(fill: white)\ncontent").unwrap();
        assert!(opaque.contains("fill=\"#ffffff\""), "{opaque}");
    }

    // [spec:mw:req:typst.preamble.svg-auto-page/test]
    #[test]
    fn compiled_pages_merge_into_one_svg_with_four_points_of_padding() {
        assert_eq!(SVG_PADDING_PT, 4.0);

        let two_pages =
            render("#set page(width: 50pt, height: 30pt, margin: 0pt)\na\n#pagebreak()\nb")
                .unwrap();
        assert_eq!(
            two_pages.matches("<svg").count(),
            1,
            "pages were not merged: {two_pages}"
        );
        // Two 30pt pages stacked with 4pt between them.
        assert_eq!(svg_root_length(&two_pages, "height"), 64.0);
        assert_eq!(svg_root_length(&two_pages, "width"), 50.0);
    }

    /// Parse a `pt`-suffixed length attribute off the `<svg>` root.
    fn svg_root_length(svg: &str, attribute: &str) -> f64 {
        let needle = format!("{attribute}=\"");
        let start = svg.find(&needle).expect("attribute present") + needle.len();
        let rest = &svg[start..];
        let end = rest.find('"').expect("closing quote");
        rest[..end]
            .strip_suffix("pt")
            .expect("pt-suffixed length")
            .parse()
            .expect("numeric length")
    }
}
