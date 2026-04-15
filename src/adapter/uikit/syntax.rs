//! Convert arborium spans into a coloured `NSAttributedString` for code blocks.
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

use arborium::advanced::Span;

use crate::adapter::uikit::style::UIKitStyle;

/// Build an attributed string for the given `source`, applying foreground
/// colors per `Span`. Returns `None` if the source has no characters.
pub(crate) fn build_highlighted(
    source: &str,
    spans: &[Span],
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

    // Apply per-span colors. Convert byte offsets to UTF-16 indices.
    let trimmed_bytes = trimmed.len();
    for span in spans {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > trimmed_bytes {
            continue;
        }
        let Some(color) = syntax_color(style, &span.capture) else {
            continue;
        };
        let utf16_start = utf16_offset(trimmed, start);
        let utf16_end = utf16_offset(trimmed, end);
        if utf16_end <= utf16_start {
            continue;
        }
        let range = NSRange::new(utf16_start, utf16_end - utf16_start);
        unsafe {
            result.addAttribute_value_range(ns_string!("NSColor"), &color, range);
        }
    }

    result
}

/// Map an arborium tree-sitter capture name (e.g. `"keyword.function"`,
/// `"string"`, `"comment"`) to a UIColor. Falls back through dotted prefixes:
/// `"keyword.function"` → tries `"keyword.function"`, then `"keyword"`.
fn syntax_color(style: &UIKitStyle, capture: &str) -> Option<Retained<UIColor>> {
    let mut name = capture;
    loop {
        if let Some(color) = capture_default_color(name) {
            return Some(color);
        }
        match name.rfind('.') {
            Some(i) => name = &name[..i],
            None => break,
        }
    }
    let _ = style; // reserved for future per-style overrides
    None
}

fn capture_default_color(capture: &str) -> Option<Retained<UIColor>> {
    let color = match capture {
        "keyword" | "include" | "storageclass" | "conditional" | "repeat" => {
            UIColor::systemPurpleColor()
        }
        "function" | "function.builtin" | "function.macro" | "method" => UIColor::systemBlueColor(),
        "type" | "type.builtin" | "namespace" | "module" | "class" => UIColor::systemTealColor(),
        "string" | "string.special" | "character" => UIColor::systemRedColor(),
        "comment" => UIColor::systemGrayColor(),
        "constant" | "constant.builtin" | "boolean" | "number" | "float" => {
            UIColor::systemOrangeColor()
        }
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => {
            return None;
        }
        "variable" | "variable.parameter" | "variable.builtin" => return None,
        "attribute" | "tag" | "tag.attribute" | "property" => UIColor::systemIndigoColor(),
        "escape" | "string.escape" => UIColor::systemYellowColor(),
        "label" => UIColor::systemPinkColor(),
        _ => return None,
    };
    Some(color)
}

fn utf16_offset(source: &str, byte_offset: usize) -> usize {
    let upper = byte_offset.min(source.len());
    source[..upper].chars().map(char::len_utf16).sum()
}
