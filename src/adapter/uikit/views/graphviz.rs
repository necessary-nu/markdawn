//! Render a graphviz DOT code block as a vector `UIImageView`.
//!
//! Pipeline: DOT source → `crate::graphviz::render_dot_to_svg` → SVG bytes →
//! `svg::render_svg` → `UIImage` → `UIImageView` constrained to its native
//! aspect ratio.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::NSString;
use objc2_ui_kit::{NSObjectUIAccessibility, UIImageView, UIView, UIViewContentMode};

use crate::adapter::uikit::svg;

/// Try to render `source` as a graphviz diagram. Returns `None` on parse /
/// layout failure or SVG rasterisation failure so the caller can fall back
/// to a plain code block view.
// [spec:mw:req:uikit.diagram.view-factory]
pub(crate) fn try_build_graphviz_view(
    source: &str,
    mtm: MainThreadMarker,
) -> Option<Retained<UIView>> {
    let svg_str = markdawn_graphviz::render_dot_to_svg(source).ok()?;
    let image = svg::render_svg(svg_str.as_bytes())?;

    let size = unsafe { image.size() };
    let image_view = UIImageView::initWithImage(UIImageView::alloc(mtm), Some(&image));
    image_view.setContentMode(UIViewContentMode::ScaleAspectFit);
    image_view.setTranslatesAutoresizingMaskIntoConstraints(false);

    if size.width > 0.0 && size.height > 0.0 {
        let ratio = size.height / size.width;
        image_view
            .heightAnchor()
            .constraintEqualToAnchor_multiplier(&image_view.widthAnchor(), ratio)
            .setActive(true);
    }

    let label = NSString::from_str("Graphviz diagram");
    image_view.setAccessibilityLabel(Some(&label), mtm);

    let view: Retained<UIView> = image_view.into_super();
    Some(view)
}
