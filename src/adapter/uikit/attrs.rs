use objc2::Message;
use objc2::rc::Retained;
use objc2_foundation::{NSMutableAttributedString, NSRange, NSString, ns_string};
use objc2_ui_kit::{
    NSMutableParagraphStyle, UIColor, UIFont, UIFontDescriptorSymbolicTraits, UIFontMetrics,
};

use super::style::UIKitStyle;

fn font_named_or<F>(name: Option<&String>, size: f64, fallback: F) -> Retained<UIFont>
where
    F: FnOnce() -> Retained<UIFont>,
{
    if let Some(name) = name {
        let name = NSString::from_str(name);
        UIFont::fontWithName_size(&name, size).unwrap_or_else(fallback)
    } else {
        fallback()
    }
}

fn maybe_scale_font(font: Retained<UIFont>, style: &UIKitStyle) -> Retained<UIFont> {
    if style.use_dynamic_type {
        UIFontMetrics::defaultMetrics().scaledFontForFont(&font)
    } else {
        font
    }
}

pub(crate) fn body_font(style: &UIKitStyle) -> Retained<UIFont> {
    body_font_with_size(style, style.body_font_size)
}

pub(crate) fn body_font_with_size(style: &UIKitStyle, size: f64) -> Retained<UIFont> {
    let font = font_named_or(style.body_font_name.as_ref(), size, || {
        UIFont::systemFontOfSize(size)
    });
    maybe_scale_font(font, style)
}

pub(crate) fn italic_font(base: &UIFont) -> Retained<UIFont> {
    let size = unsafe { base.pointSize() };
    let descriptor = unsafe { base.fontDescriptor() };
    let traits = unsafe { descriptor.symbolicTraits() };
    let italic_traits =
        UIFontDescriptorSymbolicTraits(traits.0 | UIFontDescriptorSymbolicTraits::TraitItalic.0);
    if let Some(italic_desc) = descriptor.fontDescriptorWithSymbolicTraits(italic_traits) {
        UIFont::fontWithDescriptor_size(&italic_desc, size)
    } else {
        UIFont::italicSystemFontOfSize(size)
    }
}

pub(crate) fn monospaced_font(style: &UIKitStyle, size: f64) -> Retained<UIFont> {
    let font = font_named_or(style.code_font_name.as_ref(), size, || {
        UIFont::monospacedSystemFontOfSize_weight(size, 0.0)
    });
    maybe_scale_font(font, style)
}

/// Build a monospaced font that matches the pointSize of an already-scaled
/// base font, without re-applying Dynamic Type scaling. Use this for inline
/// code spans inside already-scaled text (paragraphs, headings, etc.).
pub(crate) fn monospaced_font_matching(style: &UIKitStyle, base: &UIFont) -> Retained<UIFont> {
    let size = unsafe { base.pointSize() };
    font_named_or(style.code_font_name.as_ref(), size, || {
        UIFont::monospacedSystemFontOfSize_weight(size, 0.0)
    })
}

pub(crate) fn heading_font(style: &UIKitStyle, level: u8) -> Retained<UIFont> {
    let idx = (level.saturating_sub(1) as usize).min(5);
    let size = style.body_font_size * style.heading_scale[idx];
    let base = body_font_with_size(style, size);
    add_trait_to_font(&base, UIFontDescriptorSymbolicTraits::TraitBold)
}

pub(crate) fn add_trait_to_font(
    font: &UIFont,
    trait_to_add: UIFontDescriptorSymbolicTraits,
) -> Retained<UIFont> {
    let size = unsafe { font.pointSize() };
    let descriptor = unsafe { font.fontDescriptor() };
    let traits = unsafe { descriptor.symbolicTraits() };
    let new_traits = UIFontDescriptorSymbolicTraits(traits.0 | trait_to_add.0);
    if let Some(new_desc) = descriptor.fontDescriptorWithSymbolicTraits(new_traits) {
        UIFont::fontWithDescriptor_size(&new_desc, size)
    } else {
        font.retain()
    }
}

// --- Paragraph style helpers ---

/// Apply blockquote indentation to a paragraph style if depth > 0.
fn apply_bq_indent(ps: &NSMutableParagraphStyle, style: &UIKitStyle, bq_depth: u32) {
    if bq_depth > 0 {
        let offset = style.blockquote_indent * bq_depth as f64;
        ps.setFirstLineHeadIndent(ps.firstLineHeadIndent() + offset);
        ps.setHeadIndent(ps.headIndent() + offset);
    }
}

pub(crate) fn paragraph_style_body(
    style: &UIKitStyle,
    bq_depth: u32,
    is_last_block: bool,
) -> Retained<NSMutableParagraphStyle> {
    let ps = NSMutableParagraphStyle::new();
    ps.setParagraphSpacing(if is_last_block {
        0.0
    } else {
        style.paragraph_spacing
    });
    apply_bq_indent(&ps, style, bq_depth);
    ps
}

pub(crate) fn paragraph_style_heading(
    style: &UIKitStyle,
    level: u8,
    bq_depth: u32,
    is_last_block: bool,
) -> Retained<NSMutableParagraphStyle> {
    let idx = (level.saturating_sub(1) as usize).min(5);
    let ps = NSMutableParagraphStyle::new();
    ps.setParagraphSpacingBefore(style.heading_spacing_before[idx]);
    ps.setParagraphSpacing(if is_last_block {
        0.0
    } else {
        style.heading_spacing_after[idx]
    });
    apply_bq_indent(&ps, style, bq_depth);
    ps
}

pub(crate) fn paragraph_style_list_item(
    style: &UIKitStyle,
    depth: u32,
    tight: bool,
    is_last_block: bool,
    bq_depth: u32,
) -> Retained<NSMutableParagraphStyle> {
    let content_indent = style.list_indent * depth as f64;
    let marker_indent = style.list_indent * (depth.saturating_sub(1)) as f64;
    let ps = NSMutableParagraphStyle::new();
    ps.setFirstLineHeadIndent(marker_indent);
    ps.setHeadIndent(content_indent);
    ps.setParagraphSpacing(if is_last_block {
        0.0
    } else if tight {
        style.tight_list_spacing
    } else {
        style.paragraph_spacing
    });
    apply_bq_indent(&ps, style, bq_depth);
    ps
}

pub(crate) fn apply_paragraph_style(
    target: &NSMutableAttributedString,
    para_style: &NSMutableParagraphStyle,
) {
    let len = target.length();
    if len > 0 {
        let range = NSRange::new(0, len);
        unsafe {
            target.addAttribute_value_range(ns_string!("NSParagraphStyle"), para_style, range);
        }
    }
}

pub(crate) fn apply_background_color(target: &NSMutableAttributedString, color: &UIColor) {
    let len = target.length();
    if len > 0 {
        let range = NSRange::new(0, len);
        unsafe {
            target.addAttribute_value_range(ns_string!("NSBackgroundColor"), color, range);
        }
    }
}
