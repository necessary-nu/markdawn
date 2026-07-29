//! Lazily-loaded pikchr render module for the vdom/Preact frontend.
//!
//! Built as its own wasm so the core module carries neither pikchr nor
//! the bundled fonts its SVG text-outlining pulls in. The frontend
//! `import()`s this only when a `pikchr` code block appears.

use wasm_bindgen::prelude::*;

/// Render a pikchr diagram source to a sanitized, font-outlined SVG —
/// byte-identical to the inline pikchr blocks the HTML/vdom adapters
/// produce. Returns `undefined` on parse failure or oversize output so
/// the caller can fall back to showing the source.
// [spec:mw:sem:ffi.render-shim.lazy-module+1]
#[wasm_bindgen]
pub fn render(source: &str) -> Option<String> {
    markdawn::render_pikchr_to_svg(source)
}
