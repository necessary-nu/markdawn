//! Shared construction for the diagram code-block views.
//!
//! Every diagram engine reaches UIKit the same way: the engine's SVG has
//! already been through the crate's size cap, sanitiser and text outlining
//! by the time it arrives here, so it goes to the plain rasteriser and then
//! into an image view constrained to the diagram's native aspect ratio. Only
//! the engine and the accessibility label differ per format.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::NSString;
use objc2_ui_kit::{NSObjectUIAccessibility, UIImageView, UIView, UIViewContentMode};

use crate::adapter::uikit::svg;

/// Rasterise already-finalised SVG and wrap it in an aspect-constrained,
/// accessibility-labelled image view. `None` when rasterisation fails, so the
/// caller can fall back to a plain code block view.
///
/// The aspect-ratio constraint is skipped for a degenerate image size, since a
/// zero width would make the multiplier a division by zero.
// [spec:mw:req:uikit.diagram.view-factory+1]
pub(crate) fn diagram_image_view(
    svg_bytes: &[u8],
    accessibility_label: &str,
    mtm: MainThreadMarker,
) -> Option<Retained<UIView>> {
    let image = svg::render_svg(svg_bytes)?;

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

    let label = NSString::from_str(accessibility_label);
    image_view.setAccessibilityLabel(Some(&label), mtm);

    Some(image_view.into_super())
}
