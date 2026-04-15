use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_ui_kit::{NSObjectUIAccessibility, UIColor, UIView};

use crate::adapter::uikit::style::UIKitStyle;

/// Hairline divider for `Node::ThematicBreak` (markdown `---`).
pub fn build_thematic_break_view(_style: &UIKitStyle, mtm: MainThreadMarker) -> Retained<UIView> {
    let view = UIView::new(mtm);
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.setBackgroundColor(Some(&UIColor::separatorColor()));
    view.setIsAccessibilityElement(false, mtm);
    view.heightAnchor()
        .constraintEqualToConstant(1.0)
        .setActive(true);
    view
}
