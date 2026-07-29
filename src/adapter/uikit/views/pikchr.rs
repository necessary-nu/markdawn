//! Render a pikchr code block as a vector `UIImageView`.
//!
//! Pipeline: pikchr source → `crate::render_pikchr_to_svg` (engine
//! input cap + timeout + panic capture, then the shared output size cap
//! + sanitiser + text outlining every adapter uses) → SVG bytes →
//! `svg::render_svg` → `UIImage` → `UIImageView` constrained to its
//! native aspect ratio.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_ui_kit::UIView;

use crate::adapter::uikit::views::diagram_image_view;

/// Try to render `source` as a pikchr diagram. Returns `None` on parse /
/// layout failure, oversize engine output, or SVG rasterisation failure so
/// the caller can fall back to a plain code block view.
// [spec:mw:req:uikit.diagram.view-factory+1]
pub(crate) fn try_build_pikchr_view(
    source: &str,
    mtm: MainThreadMarker,
) -> Option<Retained<UIView>> {
    let svg_str = crate::render_pikchr_to_svg(source)?;
    diagram_image_view(svg_str.as_bytes(), "Pikchr diagram", mtm)
}
