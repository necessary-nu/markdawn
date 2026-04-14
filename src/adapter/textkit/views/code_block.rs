use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::NSString;
use objc2_ui_kit::{UILabel, UIView};

use crate::adapter::textkit::attrs;
use crate::adapter::textkit::style::TextKitStyle;

const INSET: f64 = 12.0;
const CORNER_RADIUS: f64 = 8.0;

pub fn build_code_block_view(
    literal: &str,
    _language: Option<&str>,
    style: &TextKitStyle,
    available_width: f64,
    mtm: MainThreadMarker,
) -> Retained<UIView> {
    let font = attrs::monospaced_font(style, style.body_font_size);
    let line_count = literal.lines().count().max(1);
    let line_height = style.body_font_size * 1.4;
    let content_height = line_count as f64 * line_height;
    let total_height = content_height + INSET * 2.0;

    let container = UIView::initWithFrame(
        UIView::alloc(mtm),
        CGRect::new(
            CGPoint::ZERO,
            CGSize {
                width: available_width,
                height: total_height,
            },
        ),
    );
    container.setBackgroundColor(Some(&style.code_background_color));
    unsafe {
        let layer = container.layer();
        layer.setCornerRadius(CORNER_RADIUS);
    }

    let label = UILabel::initWithFrame(
        UILabel::alloc(mtm),
        CGRect::new(
            CGPoint::new(INSET, INSET),
            CGSize {
                width: available_width - INSET * 2.0,
                height: content_height,
            },
        ),
    );
    let ns_text = NSString::from_str(literal.trim_end_matches('\n'));
    label.setText(Some(&ns_text));
    unsafe {
        label.setFont(Some(&font));
        label.setTextColor(Some(&style.code_text_color));
    }
    label.setNumberOfLines(0);
    container.addSubview(&label);

    container
}

pub fn compute_code_block_height(literal: &str, style: &TextKitStyle) -> f64 {
    let line_count = literal.lines().count().max(1);
    let line_height = style.body_font_size * 1.4;
    line_count as f64 * line_height + INSET * 2.0
}
