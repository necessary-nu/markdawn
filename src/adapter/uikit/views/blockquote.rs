use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSAttributedString, NSString};
use objc2_ui_kit::{NSObjectUIAccessibility, UITextView, UIView};

use crate::adapter::uikit::style::UIKitStyle;

const BAR_WIDTH: f64 = 3.0;
const BAR_LEFT_MARGIN: f64 = 4.0;
const CONTENT_LEFT_MARGIN: f64 = 16.0;

// [spec:mw:req:uikit.layout.blockquote]
pub fn build_blockquote_view(
    content: &NSAttributedString,
    style: &UIKitStyle,
    mtm: MainThreadMarker,
) -> Retained<UIView> {
    let container = UIView::new(mtm);
    container.setTranslatesAutoresizingMaskIntoConstraints(false);

    // Accent bar — left, fixed width, full height of container
    let bar = UIView::new(mtm);
    bar.setTranslatesAutoresizingMaskIntoConstraints(false);
    bar.setBackgroundColor(Some(&style.blockquote_accent_color));
    bar.layer().setCornerRadius(BAR_WIDTH / 2.0);
    container.addSubview(&bar);

    // Content text view — pinned right of the bar
    let text_view = UITextView::initWithFrame_textContainer(
        UITextView::alloc(mtm),
        objc2_core_foundation::CGRect::ZERO,
        None,
    );
    text_view.setTranslatesAutoresizingMaskIntoConstraints(false);
    text_view.setAttributedText(Some(content));
    text_view.setEditable(false);
    text_view.setScrollEnabled(false);
    text_view.setBackgroundColor(None);
    container.addSubview(&text_view);

    // Bar: leading margin, top-bottom to container, width
    bar.leadingAnchor()
        .constraintEqualToAnchor_constant(&container.leadingAnchor(), BAR_LEFT_MARGIN)
        .setActive(true);
    bar.topAnchor()
        .constraintEqualToAnchor(&container.topAnchor())
        .setActive(true);
    bar.bottomAnchor()
        .constraintEqualToAnchor(&container.bottomAnchor())
        .setActive(true);
    bar.widthAnchor()
        .constraintEqualToConstant(BAR_WIDTH)
        .setActive(true);

    // Text view: starts after CONTENT_LEFT_MARGIN from container leading, pinned right/top/bottom
    text_view
        .leadingAnchor()
        .constraintEqualToAnchor_constant(&container.leadingAnchor(), CONTENT_LEFT_MARGIN)
        .setActive(true);
    text_view
        .topAnchor()
        .constraintEqualToAnchor(&container.topAnchor())
        .setActive(true);
    container
        .trailingAnchor()
        .constraintEqualToAnchor(&text_view.trailingAnchor())
        .setActive(true);
    container
        .bottomAnchor()
        .constraintEqualToAnchor(&text_view.bottomAnchor())
        .setActive(true);

    container.setAccessibilityLabel(Some(&NSString::from_str("Block quote")), mtm);

    container
}
