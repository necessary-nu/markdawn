//! Convert neutral syntax spans into a coloured `NSAttributedString`.
//!
//! The mapping from tree-sitter capture names to UIColors uses iOS system
//! colours so the result adapts to light/dark mode automatically. Unknown
//! captures fall through to the base text color.

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_foundation::{
    NSAttributedString, NSMutableAttributedString, NSRange, NSString, ns_string,
};
use objc2_ui_kit::{UIColor, UIFont};

use crate::SyntaxSpan;
use crate::adapter::uikit::style::UIKitStyle;
use crate::highlight::{SyntaxAccent, capture_accent, order_spans, utf16_range};

/// Build an attributed string for the given `source`, applying foreground
/// colors per `SyntaxSpan`. Returns `None` if the source has no characters.
// [spec:mw:sem:uikit.syntax.span-priority]
// [spec:mw:req:uikit.syntax.span-bounds]
pub(crate) fn build_highlighted(
    source: &str,
    spans: &[SyntaxSpan],
    style: &UIKitStyle,
    base_font: &UIFont,
    base_color: &UIColor,
) -> Retained<NSMutableAttributedString> {
    let trimmed = source.trim_end_matches('\n');
    let ns_string = NSString::from_str(trimmed);
    let result = unsafe {
        let plain = NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &ns_string,
            None,
        );
        NSMutableAttributedString::initWithAttributedString(
            NSMutableAttributedString::alloc(),
            &plain,
        )
    };

    // Apply base font and base color across the whole string.
    let total_len = trimmed.encode_utf16().count();
    if total_len > 0 {
        let full_range = NSRange::new(0, total_len);
        unsafe {
            result.addAttribute_value_range(ns_string!("NSFont"), base_font, full_range);
            result.addAttribute_value_range(ns_string!("NSColor"), base_color, full_range);
        }
    }

    // Apply outer and lower-priority spans first so nested and higher-priority
    // captures win when attributed ranges overlap.
    for span in order_spans(spans) {
        let Some((utf16_start, utf16_len)) = utf16_range(trimmed, span) else {
            continue;
        };
        let Some(color) = syntax_color(style, &span.capture) else {
            continue;
        };
        let range = NSRange::new(utf16_start, utf16_len);
        unsafe {
            result.addAttribute_value_range(ns_string!("NSColor"), &color, range);
        }
    }

    result
}

/// Map a raw syntax capture name (e.g. `"keyword.function"`, `"string"`,
/// `"comment"`) to a UIColor. The dotted-prefix walk that resolves the
/// name to an accent is platform-neutral and lives in
/// [`crate::highlight`]; only the colour lookup is UIKit's. An accent
/// the walk cannot resolve leaves the run's colour untouched.
// [spec:mw:req:uikit.syntax.capture-fallback]
fn syntax_color(style: &UIKitStyle, capture: &str) -> Option<Retained<UIColor>> {
    let _ = style; // reserved for future per-style overrides
    let color = match capture_accent(capture)? {
        SyntaxAccent::Keyword => UIColor::systemPurpleColor(),
        SyntaxAccent::Function => UIColor::systemBlueColor(),
        SyntaxAccent::Type => UIColor::systemTealColor(),
        SyntaxAccent::StringLiteral => UIColor::systemRedColor(),
        SyntaxAccent::Comment => UIColor::systemGrayColor(),
        SyntaxAccent::Constant => UIColor::systemOrangeColor(),
        SyntaxAccent::Attribute => UIColor::systemIndigoColor(),
        SyntaxAccent::Escape => UIColor::systemYellowColor(),
        SyntaxAccent::Label => UIColor::systemPinkColor(),
    };
    Some(color)
}
