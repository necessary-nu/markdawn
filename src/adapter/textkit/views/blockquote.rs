use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::NSAttributedString;
use objc2_ui_kit::{UITextView, UIView};

use crate::adapter::textkit::style::TextKitStyle;

const BAR_WIDTH: f64 = 3.0;
const BAR_LEFT_MARGIN: f64 = 4.0;
const CONTENT_LEFT_MARGIN: f64 = 16.0;

pub fn build_blockquote_view(
    content: &NSAttributedString,
    style: &TextKitStyle,
    available_width: f64,
    mtm: MainThreadMarker,
) -> Retained<UIView> {
    let content_width = available_width - CONTENT_LEFT_MARGIN;

    // Estimate height from content length (rough heuristic)
    let text_len = content.length() as f64;
    let chars_per_line = (content_width / (style.body_font_size * 0.5)).max(1.0);
    let line_count = (text_len / chars_per_line).ceil().max(1.0);
    let line_height = style.body_font_size * 1.5;
    let content_height = line_count * line_height;

    let container = UIView::initWithFrame(
        UIView::alloc(mtm),
        CGRect::new(
            CGPoint::ZERO,
            CGSize {
                width: available_width,
                height: content_height,
            },
        ),
    );

    // Accent bar
    let bar = UIView::initWithFrame(
        UIView::alloc(mtm),
        CGRect::new(
            CGPoint::new(BAR_LEFT_MARGIN, 0.0),
            CGSize {
                width: BAR_WIDTH,
                height: content_height,
            },
        ),
    );
    bar.setBackgroundColor(Some(&style.blockquote_accent_color));
    unsafe {
        bar.layer().setCornerRadius(BAR_WIDTH / 2.0);
    }
    container.addSubview(&bar);

    // Content text view
    let text_view = UITextView::initWithFrame_textContainer(
        UITextView::alloc(mtm),
        CGRect::new(
            CGPoint::new(CONTENT_LEFT_MARGIN, 0.0),
            CGSize {
                width: content_width,
                height: content_height,
            },
        ),
        None,
    );
    text_view.setAttributedText(Some(content));
    text_view.setEditable(false);
    text_view.setScrollEnabled(false);
    text_view.setBackgroundColor(None);
    container.addSubview(&text_view);

    container
}

pub fn compute_blockquote_height(
    content: &NSAttributedString,
    style: &TextKitStyle,
    available_width: f64,
) -> f64 {
    let content_width = available_width - CONTENT_LEFT_MARGIN;
    let text_len = content.length() as f64;
    let chars_per_line = (content_width / (style.body_font_size * 0.5)).max(1.0);
    let line_count = (text_len / chars_per_line).ceil().max(1.0);
    let line_height = style.body_font_size * 1.5;
    line_count * line_height
}
