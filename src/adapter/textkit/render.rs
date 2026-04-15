//! Per-block attributed-string rendering for text-like markdown nodes.
//!
//! Block-level nodes that map to standalone UIViews (Table, CodeBlock, BlockQuote)
//! are NOT rendered here — `BlockSink` dispatches those directly to view factories
//! in `crate::adapter::textkit::views`.
//!
//! Inline SVG (within text content) is rendered as an `NSTextAttachment` with
//! `setImage`, which is the one place attachments behave reliably.

use std::sync::Arc;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_foundation::{
    NSAttributedString, NSMutableAttributedString, NSNumber, NSRange, NSString, ns_string,
};
use objc2_ui_kit::{
    NSAttributedStringAttachmentConveniences, NSTextAttachment, UIColor, UIFont,
    UIFontDescriptorSymbolicTraits,
};

use crate::node::Node;

use super::attrs;
use super::style::TextKitStyle;
use super::svg;

pub struct RenderCtx {
    pub blockquote_depth: u32,
    pub list_stack: Vec<ListFrame>,
    pub provisional: bool,
    pub is_last_list_item: bool,
    pub is_last_block: bool,
}

impl Default for RenderCtx {
    fn default() -> Self {
        Self {
            blockquote_depth: 0,
            list_stack: Vec::new(),
            provisional: false,
            is_last_list_item: false,
            is_last_block: false,
        }
    }
}

#[derive(Clone)]
pub struct ListFrame {
    pub ordered: bool,
    pub start: u64,
    pub item_index: u32,
    pub tight: bool,
}

pub trait RenderTextKitCustom<Ext, Meta> {
    fn render_custom(
        &self,
        payload: &Ext,
        children: &[Node<Ext, Meta>],
        style: &TextKitStyle,
        ctx: &RenderCtx,
    ) -> Result<Retained<NSMutableAttributedString>, TextKitError>;
}

impl<Ext, Meta> RenderTextKitCustom<Ext, Meta> for () {
    fn render_custom(
        &self,
        _payload: &Ext,
        _children: &[Node<Ext, Meta>],
        _style: &TextKitStyle,
        _ctx: &RenderCtx,
    ) -> Result<Retained<NSMutableAttributedString>, TextKitError> {
        Err(TextKitError::UnsupportedCustomNode)
    }
}

#[derive(Debug)]
pub enum TextKitError {
    UnsupportedCustomNode,
    UnsupportedBlockNode,
}

impl core::fmt::Display for TextKitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedCustomNode => f.write_str("custom node renderer not configured"),
            Self::UnsupportedBlockNode => f.write_str("non-text block passed to text renderer"),
        }
    }
}

impl std::error::Error for TextKitError {}

/// Render text-like markdown blocks (Paragraph, Heading, List, ListItem, ThematicBreak,
/// Document container) to an attributed string. Non-text blocks (Table, CodeBlock,
/// BlockQuote, block-level Html/SVG) return `Err(UnsupportedBlockNode)`.
pub struct TextRenderer<R = ()> {
    pub(crate) style: Arc<TextKitStyle>,
    pub(crate) custom: R,
}

impl Default for TextRenderer<()> {
    fn default() -> Self {
        Self::new(Arc::new(TextKitStyle::default()))
    }
}

impl TextRenderer<()> {
    pub fn new(style: Arc<TextKitStyle>) -> Self {
        Self { style, custom: () }
    }
}

impl<R> TextRenderer<R> {
    pub fn with_custom(style: Arc<TextKitStyle>, custom: R) -> Self {
        Self { style, custom }
    }

    pub fn style(&self) -> &Arc<TextKitStyle> {
        &self.style
    }

    pub fn render_text_block<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        ctx: &RenderCtx,
    ) -> Result<Retained<NSMutableAttributedString>, TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let root_ctx = RenderCtx {
            blockquote_depth: ctx.blockquote_depth,
            list_stack: ctx.list_stack.clone(),
            provisional: ctx.provisional,
            is_last_list_item: ctx.is_last_list_item,
            is_last_block: true,
        };
        self.render_block_inner(node, &root_ctx)
    }

    fn render_block_inner<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        ctx: &RenderCtx,
    ) -> Result<Retained<NSMutableAttributedString>, TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let result = NSMutableAttributedString::new();
        let base_font = attrs::body_font(&self.style);
        let text_color = if ctx.provisional {
            self.style
                .text_color
                .colorWithAlphaComponent(self.style.provisional_alpha as f64)
        } else {
            self.style.text_color.clone()
        };

        match node {
            Node::Document { children, .. } => {
                let last_idx = children.len().saturating_sub(1);
                for (i, child) in children.iter().enumerate() {
                    let child_ctx = RenderCtx {
                        blockquote_depth: ctx.blockquote_depth,
                        list_stack: ctx.list_stack.clone(),
                        provisional: ctx.provisional,
                        is_last_list_item: false,
                        is_last_block: i == last_idx,
                    };
                    let child_str = self.render_block_inner(child, &child_ctx)?;
                    result.appendAttributedString(&child_str);
                }
            }
            Node::Paragraph { children, .. } => {
                let inlines = self.render_inlines(children, &base_font, &text_color, ctx)?;
                result.appendAttributedString(&inlines);
                append_plain(&result, "\n", &base_font, &text_color);
                let ps = attrs::paragraph_style_body(
                    &self.style,
                    ctx.blockquote_depth,
                    ctx.is_last_block,
                );
                attrs::apply_paragraph_style(&result, &ps);
            }
            Node::Heading {
                level, children, ..
            } => {
                let font = attrs::heading_font(&self.style, *level);
                let inlines = self.render_inlines(children, &font, &text_color, ctx)?;
                result.appendAttributedString(&inlines);
                append_plain(&result, "\n", &font, &text_color);
                let ps = attrs::paragraph_style_heading(
                    &self.style,
                    *level,
                    ctx.blockquote_depth,
                    ctx.is_last_block,
                );
                attrs::apply_paragraph_style(&result, &ps);
            }
            Node::List {
                ordered,
                start,
                tight,
                children,
                ..
            } => {
                let frame = ListFrame {
                    ordered: *ordered,
                    start: start.unwrap_or(1),
                    item_index: 0,
                    tight: tight.unwrap_or(true),
                };
                let mut inner_ctx = RenderCtx {
                    blockquote_depth: ctx.blockquote_depth,
                    list_stack: {
                        let mut s = ctx.list_stack.clone();
                        s.push(frame);
                        s
                    },
                    provisional: ctx.provisional,
                    is_last_list_item: false,
                    is_last_block: false,
                };
                let last_idx = children.len().saturating_sub(1);
                for (i, child) in children.iter().enumerate() {
                    if let Some(frame) = inner_ctx.list_stack.last_mut() {
                        frame.item_index = i as u32;
                    }
                    inner_ctx.is_last_list_item = i == last_idx;
                    inner_ctx.is_last_block = ctx.is_last_block && i == last_idx;
                    let child_str = self.render_block_inner(child, &inner_ctx)?;
                    result.appendAttributedString(&child_str);
                }
            }
            Node::Item { children, .. } => {
                let prefix = self.list_item_prefix(ctx);
                append_plain(&result, &prefix, &base_font, &text_color);
                for child in children {
                    match child {
                        Node::Paragraph {
                            children: inlines, ..
                        } => {
                            let rendered =
                                self.render_inlines(inlines, &base_font, &text_color, ctx)?;
                            result.appendAttributedString(&rendered);
                        }
                        other => {
                            let rendered = self.render_block_inner(other, ctx)?;
                            result.appendAttributedString(&rendered);
                        }
                    }
                }
                append_plain(&result, "\n", &base_font, &text_color);
                let tight = ctx.list_stack.last().map_or(false, |f| f.tight);
                let is_last = ctx.is_last_list_item;
                let depth = ctx.list_stack.len() as u32;
                let ps = attrs::paragraph_style_list_item(
                    &self.style,
                    depth,
                    tight && !is_last,
                    ctx.is_last_block,
                    ctx.blockquote_depth,
                );
                attrs::apply_paragraph_style(&result, &ps);
            }
            Node::TaskItem {
                checked, children, ..
            } => {
                let checkbox = if *checked { "\u{2611} " } else { "\u{2610} " };
                let prefix = self.list_item_prefix(ctx);
                append_plain(&result, &prefix, &base_font, &text_color);
                append_plain(&result, checkbox, &base_font, &text_color);
                for child in children {
                    match child {
                        Node::Paragraph {
                            children: inlines, ..
                        } => {
                            let rendered =
                                self.render_inlines(inlines, &base_font, &text_color, ctx)?;
                            result.appendAttributedString(&rendered);
                        }
                        other => {
                            let rendered = self.render_block_inner(other, ctx)?;
                            result.appendAttributedString(&rendered);
                        }
                    }
                }
                append_plain(&result, "\n", &base_font, &text_color);
                let tight = ctx.list_stack.last().map_or(false, |f| f.tight);
                let is_last = ctx.is_last_list_item;
                let depth = ctx.list_stack.len() as u32;
                let ps = attrs::paragraph_style_list_item(
                    &self.style,
                    depth,
                    tight && !is_last,
                    ctx.is_last_block,
                    ctx.blockquote_depth,
                );
                attrs::apply_paragraph_style(&result, &ps);
            }
            Node::ThematicBreak { .. } => {
                append_plain(
                    &result,
                    "\u{2500}\u{2500}\u{2500}\n",
                    &base_font,
                    &text_color,
                );
                let ps = attrs::paragraph_style_body(
                    &self.style,
                    ctx.blockquote_depth,
                    ctx.is_last_block,
                );
                attrs::apply_paragraph_style(&result, &ps);
            }
            Node::Custom {
                payload, children, ..
            } => {
                let custom_str = self
                    .custom
                    .render_custom(payload, children, &self.style, ctx)?;
                result.appendAttributedString(&custom_str);
            }
            Node::Text { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. } => {
                let inline = self.render_inline(node, &base_font, &text_color, ctx)?;
                result.appendAttributedString(&inline);
                append_plain(&result, "\n", &base_font, &text_color);
            }
            Node::Html { html, .. } => {
                if let Some(svg_attachment) = try_render_svg_html(html) {
                    result.appendAttributedString(&svg_attachment);
                    append_plain(&result, "\n", &base_font, &text_color);
                } else {
                    let code_font =
                        attrs::monospaced_font(&self.style, self.style.body_font_size);
                    append_plain(&result, html, &code_font, &text_color);
                    if !html.ends_with('\n') {
                        append_plain(&result, "\n", &code_font, &text_color);
                    }
                }
            }
            // Non-text blocks should be intercepted by the sink before reaching here.
            Node::Table { .. } | Node::CodeBlock { .. } | Node::BlockQuote { .. } => {
                return Err(TextKitError::UnsupportedBlockNode);
            }
            Node::TableRow { cells, .. } => {
                // Standalone table row outside a Table — render cells as text
                for (i, cell) in cells.iter().enumerate() {
                    if i > 0 {
                        append_plain(&result, " | ", &base_font, &text_color);
                    }
                    if let Node::TableCell { children, .. } = cell {
                        let rendered =
                            self.render_inlines(children, &base_font, &text_color, ctx)?;
                        result.appendAttributedString(&rendered);
                    }
                }
                append_plain(&result, "\n", &base_font, &text_color);
            }
            Node::TableCell { children, .. } => {
                let inlines = self.render_inlines(children, &base_font, &text_color, ctx)?;
                result.appendAttributedString(&inlines);
                append_plain(&result, "\n", &base_font, &text_color);
            }
        }

        Ok(result)
    }

    pub(crate) fn render_inlines<Ext, Meta>(
        &self,
        nodes: &[Node<Ext, Meta>],
        base_font: &UIFont,
        base_color: &UIColor,
        ctx: &RenderCtx,
    ) -> Result<Retained<NSMutableAttributedString>, TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let result = NSMutableAttributedString::new();
        for node in nodes {
            let rendered = self.render_inline(node, base_font, base_color, ctx)?;
            result.appendAttributedString(&rendered);
        }
        Ok(result)
    }

    fn render_inline<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        base_font: &UIFont,
        base_color: &UIColor,
        ctx: &RenderCtx,
    ) -> Result<Retained<NSMutableAttributedString>, TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        match node {
            Node::Text { text, .. } => Ok(attributed_string(text, base_font, base_color)),
            Node::Emphasis { children, .. } => {
                let italic = attrs::italic_font(base_font);
                self.render_inlines(children, &italic, base_color, ctx)
            }
            Node::Strong { children, .. } => {
                let bold =
                    attrs::add_trait_to_font(base_font, UIFontDescriptorSymbolicTraits::TraitBold);
                self.render_inlines(children, &bold, base_color, ctx)
            }
            Node::Strikethrough { children, .. } => {
                let result = self.render_inlines(children, base_font, base_color, ctx)?;
                let range = NSRange::new(0, result.length());
                unsafe {
                    result.addAttribute_value_range(
                        ns_string!("NSStrikethrough"),
                        &NSNumber::new_i32(1),
                        range,
                    );
                }
                Ok(result)
            }
            Node::Link {
                destination,
                children,
                ..
            } => {
                let link_color = self.style.link_color.clone();
                let result = self.render_inlines(children, base_font, &link_color, ctx)?;
                let range = NSRange::new(0, result.length());
                let url_str = NSString::from_str(destination);
                unsafe {
                    result.addAttribute_value_range(ns_string!("NSLink"), &url_str, range);
                }
                Ok(result)
            }
            Node::Image { alt, .. } => {
                let display = format!("[{alt}]");
                Ok(attributed_string(&display, base_font, base_color))
            }
            Node::CodeSpan { code, .. } => {
                let mono = attrs::monospaced_font(&self.style, unsafe { base_font.pointSize() });
                let code_color = self.style.code_text_color.clone();
                let result = attributed_string(code, &mono, &code_color);
                let bg = self.style.code_background_color.clone();
                attrs::apply_background_color(&result, &bg);
                Ok(result)
            }
            Node::Html { html, .. } => {
                if let Some(svg_attachment) = try_render_svg_html(html) {
                    let result = NSMutableAttributedString::new();
                    result.appendAttributedString(&svg_attachment);
                    Ok(result)
                } else {
                    let mono =
                        attrs::monospaced_font(&self.style, unsafe { base_font.pointSize() });
                    Ok(attributed_string(html, &mono, base_color))
                }
            }
            Node::SoftBreak { .. } => Ok(attributed_string(" ", base_font, base_color)),
            Node::HardBreak { .. } => Ok(attributed_string("\n", base_font, base_color)),
            Node::Custom {
                payload, children, ..
            } => self
                .custom
                .render_custom(payload, children, &self.style, ctx),
            other => self.render_block_inner(other, ctx),
        }
    }

    /// Extract flat cell text + column widths from a table for view factory consumption.
    pub(crate) fn extract_table_data<Ext, Meta>(
        &self,
        header: &[Node<Ext, Meta>],
        rows: &[Node<Ext, Meta>],
        ctx: &RenderCtx,
    ) -> (Vec<String>, Vec<Vec<String>>, Vec<usize>)
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let base_font = attrs::body_font(&self.style);
        let text_color = self.style.text_color.clone();

        let extract_row = |row: &Node<Ext, Meta>| -> Vec<String> {
            let Node::TableRow { cells, .. } = row else {
                return Vec::new();
            };
            cells
                .iter()
                .map(|cell| {
                    if let Node::TableCell { children, .. } = cell {
                        if let Ok(rendered) =
                            self.render_inlines(children, &base_font, &text_color, ctx)
                        {
                            return rendered.string().to_string();
                        }
                    }
                    String::new()
                })
                .collect()
        };

        let header_cells: Vec<String> = header.first().map(extract_row).unwrap_or_default();
        let data_rows: Vec<Vec<String>> = rows.iter().map(|r| extract_row(r)).collect();

        // Compute column widths
        let mut col_widths = vec![0usize; header_cells.len()];
        for (i, text) in header_cells.iter().enumerate() {
            col_widths[i] = col_widths[i].max(text.len());
        }
        for row in &data_rows {
            if col_widths.len() < row.len() {
                col_widths.resize(row.len(), 0);
            }
            for (i, text) in row.iter().enumerate() {
                col_widths[i] = col_widths[i].max(text.len());
            }
        }

        (header_cells, data_rows, col_widths)
    }

    /// Render the inner content of a blockquote as an attributed string suitable
    /// for embedding in a UITextView inside the blockquote view.
    pub(crate) fn render_blockquote_content<Ext, Meta>(
        &self,
        children: &[Node<Ext, Meta>],
        ctx: &RenderCtx,
    ) -> Result<Retained<NSMutableAttributedString>, TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let inner_ctx = RenderCtx {
            blockquote_depth: 0, // view handles visual treatment
            list_stack: ctx.list_stack.clone(),
            provisional: ctx.provisional,
            is_last_list_item: false,
            is_last_block: true,
        };
        let result = NSMutableAttributedString::new();
        let last_idx = children.len().saturating_sub(1);
        for (i, child) in children.iter().enumerate() {
            let mut child_ctx = RenderCtx {
                blockquote_depth: 0,
                list_stack: inner_ctx.list_stack.clone(),
                provisional: ctx.provisional,
                is_last_list_item: false,
                is_last_block: i == last_idx,
            };
            child_ctx.is_last_block = i == last_idx;
            let child_str = self.render_block_inner(child, &child_ctx)?;
            result.appendAttributedString(&child_str);
        }
        Ok(result)
    }

    fn list_item_prefix(&self, ctx: &RenderCtx) -> String {
        if let Some(frame) = ctx.list_stack.last() {
            if frame.ordered {
                format!("{}. ", frame.start as u32 + frame.item_index)
            } else {
                "\u{2022} ".to_owned()
            }
        } else {
            String::new()
        }
    }
}

fn attributed_string(
    text: &str,
    font: &UIFont,
    color: &UIColor,
) -> Retained<NSMutableAttributedString> {
    let result = NSMutableAttributedString::new();
    append_plain(&result, text, font, color);
    result
}

/// Try to render an HTML chunk as an SVG `NSTextAttachment` (image-based, not view provider).
fn try_render_svg_html(html: &str) -> Option<Retained<NSAttributedString>> {
    if !svg::is_svg_data(html.as_bytes()) {
        return None;
    }
    let image = svg::render_svg(html.as_bytes())?;
    let attachment = NSTextAttachment::new();
    unsafe {
        attachment.setImage(Some(&image));
    }
    Some(NSAttributedString::attributedStringWithAttachment(&attachment))
}

fn append_plain(target: &NSMutableAttributedString, text: &str, font: &UIFont, color: &UIColor) {
    let ns_text = NSString::from_str(text);
    let plain =
        NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &ns_text);
    let range = NSRange::new(0, plain.length());
    unsafe {
        plain.addAttribute_value_range(ns_string!("NSFont"), font, range);
        plain.addAttribute_value_range(ns_string!("NSColor"), color, range);
    }
    target.appendAttributedString(&plain);
}
