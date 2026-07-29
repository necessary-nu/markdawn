use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::{NSAttributedString, NSString};
use objc2_ui_kit::{NSObjectUIAccessibility, UIColor, UIFont, UILabel, UIView};

use crate::adapter::uikit::attrs;
use crate::adapter::uikit::style::UIKitStyle;

const INSET: f64 = 12.0;
const CORNER_RADIUS: f64 = 8.0;
const BADGE_INSET: f64 = 6.0;

// [spec:mw:def:uikit.layout.code-block-view]
pub fn build_code_block_view(
    literal: &str,
    language: Option<&str>,
    highlighted: Option<&NSAttributedString>,
    style: &UIKitStyle,
    mtm: MainThreadMarker,
) -> Retained<UIView> {
    let font = attrs::monospaced_font(style, style.body_font_size);

    let container = UIView::new(mtm);
    container.setTranslatesAutoresizingMaskIntoConstraints(false);
    container.setBackgroundColor(Some(&style.code_background_color));
    unsafe {
        container.layer().setCornerRadius(CORNER_RADIUS);
    }

    let badge = language
        .filter(|s| !s.is_empty())
        .map(|lang| build_badge(lang, style, mtm));
    if let Some(badge) = badge.as_ref() {
        container.addSubview(badge);
        unsafe {
            badge
                .topAnchor()
                .constraintEqualToAnchor_constant(&container.topAnchor(), BADGE_INSET)
                .setActive(true);
            container
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&badge.trailingAnchor(), BADGE_INSET)
                .setActive(true);
        }
    }

    let label = UILabel::new(mtm);
    if let Some(attr) = highlighted {
        unsafe {
            label.setAttributedText(Some(attr));
        }
    } else {
        let ns_text = NSString::from_str(literal.trim_end_matches('\n'));
        label.setText(Some(&ns_text));
        unsafe {
            label.setFont(Some(&font));
            label.setTextColor(Some(&style.code_text_color));
        }
    }
    label.setNumberOfLines(0);
    label.setTranslatesAutoresizingMaskIntoConstraints(false);
    container.addSubview(&label);

    // Pin label with INSET padding on all sides; when a badge is present,
    // push the code label down so it doesn't overlap the badge.
    let top_inset = if badge.is_some() { INSET + 8.0 } else { INSET };
    unsafe {
        label
            .topAnchor()
            .constraintEqualToAnchor_constant(&container.topAnchor(), top_inset)
            .setActive(true);
        label
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&container.leadingAnchor(), INSET)
            .setActive(true);
        container
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&label.trailingAnchor(), INSET)
            .setActive(true);
        container
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&label.bottomAnchor(), INSET)
            .setActive(true);
    }

    let a11y = if let Some(lang) = language.filter(|s| !s.is_empty()) {
        NSString::from_str(&format!("Code block, {lang}"))
    } else {
        NSString::from_str("Code block")
    };
    container.setAccessibilityLabel(Some(&a11y), mtm);

    container
}

fn build_badge(language: &str, style: &UIKitStyle, mtm: MainThreadMarker) -> Retained<UILabel> {
    let label = UILabel::new(mtm);
    let ns_text = NSString::from_str(language);
    label.setText(Some(&ns_text));
    let size = (style.body_font_size * 0.75).max(10.0);
    let badge_font: Retained<UIFont> = UIFont::monospacedSystemFontOfSize_weight(size, 0.0);
    let badge_font = if style.use_dynamic_type {
        objc2_ui_kit::UIFontMetrics::defaultMetrics().scaledFontForFont(&badge_font)
    } else {
        badge_font
    };
    unsafe {
        label.setFont(Some(&badge_font));
        label.setTextColor(Some(&UIColor::tertiaryLabelColor()));
    }
    label.setTranslatesAutoresizingMaskIntoConstraints(false);
    label
}
