//! LaTeX math rendering via `mathml-core` → SVG → CoreSVG → `UIImage`.
//!
//! The bundled STIX Two Math font inside mathml-core is reused across calls
//! via `Renderer::default()` (backed by a `OnceLock`). Rendered images are
//! cached by `(hash(source), quantized_size)` so re-renders during streaming
//! don't re-run LaTeX layout + SVG → CGImage rasterization.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use mathml_core::Renderer;
use objc2::rc::Retained;
use objc2_ui_kit::UIImage;

use super::svg;

#[derive(Clone, Copy)]
pub(crate) enum MathDisplay {
    Inline,
    Block,
}

fn renderer() -> &'static Renderer {
    static RENDERER: OnceLock<Renderer> = OnceLock::new();
    RENDERER.get_or_init(Renderer::default)
}

fn cache() -> &'static Mutex<HashMap<(u64, u32), Retained<UIImage>>> {
    static CACHE: OnceLock<Mutex<HashMap<(u64, u32), Retained<UIImage>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hash_source(source: &str) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

pub(crate) fn render_latex(source: &str, size: f64) -> Option<Retained<UIImage>> {
    let key = (hash_source(source), (size * 100.0).round() as u32);

    if let Ok(guard) = cache().lock() {
        if let Some(image) = guard.get(&key) {
            return Some(image.clone());
        }
    }

    let svg = renderer()
        .clone()
        .with_font_size(size)
        .render_latex(source)
        .ok()?;
    let image = svg::render_svg(svg.as_bytes())?;

    if let Ok(mut guard) = cache().lock() {
        if guard.len() >= 200 {
            if let Some(oldest) = guard.keys().next().copied() {
                guard.remove(&oldest);
            }
        }
        guard.insert(key, image.clone());
    }

    Some(image)
}

pub(crate) fn size_for(display: MathDisplay, body_font_size: f64, block_scale: f64) -> f64 {
    match display {
        MathDisplay::Inline => body_font_size,
        MathDisplay::Block => body_font_size * block_scale,
    }
}
