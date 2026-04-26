//! Block-level sink that maintains a `UIStackView` of compartments.
//!
//! Each block from the parser becomes either:
//! - part of a `TextRun` compartment (a `UITextView` backed by `NSTextContentStorage`)
//!   if it's a text-like block (Paragraph, Heading, List, ...)
//! - its own `BlockView` compartment (a custom `UIView` from `views::*` factories)
//!   if it's a non-text block (Table, CodeBlock, BlockQuote)
//!
//! Consecutive text-like blocks share a single TextRun compartment.

use std::sync::Arc;

use objc2::rc::Retained;
use objc2::{AnyThread, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSAttributedString, NSMutableAttributedString, NSRange};
use objc2_ui_kit::{
    NSTextContainer, NSTextContentStorage, NSTextLayoutManager, NSTextStorageObserving, UIColor,
    UIEdgeInsets, UILayoutConstraintAxis, UIStackView, UITextView, UIView,
};

use crate::node::{CorrelationId, Emission, Node, StreamItem};
use crate::sink::Sink;

use super::element_map::{ElementLocation, ElementMap, shift_compartment_indices};
use super::render::{RenderCtx, RenderUIKitCustom, TextRenderer, UIKitError};
use super::style::UIKitStyle;
#[cfg(feature = "graphviz")]
use super::views::graphviz;
#[cfg(feature = "pikchr")]
use super::views::pikchr;
use super::views::{blockquote, code_block, table, thematic_break};

pub struct BlockSink<R = ()> {
    renderer: TextRenderer<R>,
    stack_view: Retained<UIStackView>,
    compartments: Vec<Compartment>,
    element_map: ElementMap,
    mtm: MainThreadMarker,
    #[cfg(feature = "highlight")]
    highlighter: Option<Box<dyn crate::highlight::SpanHighlighter>>,
    #[cfg(feature = "pikchr")]
    pikchr: bool,
    #[cfg(feature = "graphviz")]
    graphviz: bool,
    #[cfg(feature = "typst")]
    typst: bool,
}

enum Compartment {
    TextRun {
        text_view: Retained<UITextView>,
        content_storage: Retained<NSTextContentStorage>,
        blocks: Vec<TextBlockEntry>,
    },
    BlockView {
        view: Retained<UIView>,
        id: CorrelationId,
        // Some(_) for tables; lets `replace_existing` mutate cells in place
        // without removing/inserting the view (which causes the parent stack to
        // re-layout sibling text and visually jump).
        table_handles: Option<table::TableHandles>,
        /// Remote image URLs introduced by this block view (e.g. images
        /// inside table cells). Cancelled when the compartment is removed.
        image_urls: Vec<String>,
    },
}

struct TextBlockEntry {
    id: CorrelationId,
    range: NSRange,
    /// Remote image URLs introduced by this block, kept so we can cancel
    /// any in-flight downloads if the block is replaced or removed before
    /// the response arrives.
    image_urls: Vec<String>,
}

impl BlockSink<()> {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self::with_arc_style(mtm, Arc::new(UIKitStyle::default()))
    }

    pub fn with_style(mtm: MainThreadMarker, style: UIKitStyle) -> Self {
        Self::with_arc_style(mtm, Arc::new(style))
    }

    fn with_arc_style(mtm: MainThreadMarker, style: Arc<UIKitStyle>) -> Self {
        let stack_view = make_stack_view(mtm);
        Self {
            renderer: TextRenderer::new(style),
            stack_view,
            compartments: Vec::new(),
            element_map: ElementMap::new(),

            mtm,
            #[cfg(feature = "highlight")]
            highlighter: None,
            #[cfg(feature = "pikchr")]
            pikchr: false,
            #[cfg(feature = "graphviz")]
            graphviz: false,
            #[cfg(feature = "typst")]
            typst: false,
        }
    }
}

impl<R> BlockSink<R> {
    pub fn with_custom_renderer(mtm: MainThreadMarker, style: UIKitStyle, custom: R) -> Self {
        let stack_view = make_stack_view(mtm);
        Self {
            renderer: TextRenderer::with_custom(Arc::new(style), custom),
            stack_view,
            compartments: Vec::new(),
            element_map: ElementMap::new(),

            mtm,
            #[cfg(feature = "highlight")]
            highlighter: None,
            #[cfg(feature = "pikchr")]
            pikchr: false,
            #[cfg(feature = "graphviz")]
            graphviz: false,
            #[cfg(feature = "typst")]
            typst: false,
        }
    }

    /// Install a syntax highlighter for fenced code blocks. Any
    /// `arborium::advanced::HighlightSpans` implementation works; failures
    /// silently fall back to plain monospace rendering.
    #[cfg(feature = "highlight")]
    pub fn with_highlighter<H>(mut self, highlighter: H) -> Self
    where
        H: arborium::advanced::HighlightSpans + 'static,
    {
        self.highlighter = Some(Box::new(highlighter));
        self
    }

    /// Render fenced code blocks tagged with the `pikchr` info string as
    /// vector image diagrams via `pikru`. Parse failures fall back to the
    /// regular code-block view.
    #[cfg(feature = "pikchr")]
    pub fn with_pikchr(mut self) -> Self {
        self.pikchr = true;
        self
    }

    /// Render fenced code blocks tagged `dot` or `graphviz` as vector image
    /// diagrams via `graphviz-sys`. Parse / layout failures fall back to
    /// the regular code-block view. Requires libgvc / libcgraph to be
    /// linkable in the iOS build.
    #[cfg(feature = "graphviz")]
    pub fn with_graphviz(mut self) -> Self {
        self.graphviz = true;
        self
    }

    /// Render fenced code blocks tagged `typst` as vector image figures
    /// by compiling the source through the bundled Typst compiler.
    /// Compile failures fall back to the regular code-block view.
    #[cfg(feature = "typst")]
    pub fn with_typst(mut self) -> Self {
        self.typst = true;
        self
    }

    /// The `UIStackView` that arranges all rendered blocks. Embed this in your hierarchy.
    pub fn stack_view(&self) -> &UIStackView {
        &self.stack_view
    }

    pub fn mtm(&self) -> MainThreadMarker {
        self.mtm
    }

    pub fn attributed_text(&self) -> Retained<NSAttributedString> {
        let result = NSMutableAttributedString::new();
        let mut first = true;

        for compartment in &self.compartments {
            if !first {
                let separator =
                    NSAttributedString::from_nsstring(&objc2_foundation::NSString::from_str("\n"));
                result.appendAttributedString(&separator);
            }
            first = false;

            match compartment {
                Compartment::TextRun {
                    content_storage, ..
                } => {
                    if let Some(storage) = content_storage.textStorage() {
                        let rendered = NSAttributedString::initWithAttributedString(
                            NSAttributedString::alloc(),
                            storage.as_super(),
                        );
                        result.appendAttributedString(&rendered);
                    }
                }
                Compartment::BlockView { .. } => {}
            }
        }

        NSAttributedString::initWithAttributedString(NSAttributedString::alloc(), result.as_super())
    }

    fn apply_upsert<Ext>(&mut self, emission: Emission<Node<Ext>>) -> Result<(), UIKitError>
    where
        R: RenderUIKitCustom<Ext>,
    {
        let id = emission.id;
        let provisional = emission.provisional;
        let node = emission.node;

        // If this id already has a location, update in place.
        if let Some(loc) = self.element_map.get(&id).copied() {
            return self.replace_existing(id, loc, &node, provisional);
        }

        // New id: dispatch by node kind.
        if is_text_block(&node) {
            self.append_text_block(id, &node, provisional)
        } else {
            self.append_block_view(id, &node, provisional)
        }
    }

    fn append_text_block<Ext>(
        &mut self,
        id: CorrelationId,
        node: &Node<Ext>,
        provisional: bool,
    ) -> Result<(), UIKitError>
    where
        R: RenderUIKitCustom<Ext>,
    {
        let ctx = RenderCtx {
            provisional,
            ..Default::default()
        };
        let rendered = self.renderer.render_text_block(node, &ctx)?;
        let len = rendered.length();

        // Try to append to the last compartment if it's a TextRun.
        let last_idx = self.compartments.len().checked_sub(1);
        let append_to_last = matches!(
            last_idx.and_then(|i| self.compartments.get(i)),
            Some(Compartment::TextRun { .. })
        );

        let compartment_index = if append_to_last {
            let idx = last_idx.unwrap();
            let Compartment::TextRun {
                content_storage,
                blocks,
                ..
            } = &mut self.compartments[idx]
            else {
                unreachable!()
            };
            let storage = content_storage
                .textStorage()
                .expect("content storage must have textStorage");
            let insert_loc = storage.length();
            storage.appendAttributedString(&rendered);
            blocks.push(TextBlockEntry {
                id,
                range: NSRange::new(insert_loc, len),
                image_urls: Vec::new(),
            });
            self.element_map.insert(
                id,
                ElementLocation::TextRun {
                    compartment_index: idx,
                },
            );
            idx
        } else {
            let (text_view, content_storage) = make_text_run(self.mtm);
            let storage = content_storage
                .textStorage()
                .expect("content storage must have textStorage");
            storage.appendAttributedString(&rendered);

            self.stack_view.addArrangedSubview(&text_view);
            let idx = self.compartments.len();
            self.compartments.push(Compartment::TextRun {
                text_view,
                content_storage,
                blocks: vec![TextBlockEntry {
                    id,
                    range: NSRange::new(0, len),
                    image_urls: Vec::new(),
                }],
            });
            self.element_map.insert(
                id,
                ElementLocation::TextRun {
                    compartment_index: idx,
                },
            );
            idx
        };

        // Register any pending remote images with the async loader.
        self.register_pending_images(&ctx, compartment_index);

        Ok(())
    }

    fn append_block_view<Ext>(
        &mut self,
        id: CorrelationId,
        node: &Node<Ext>,
        provisional: bool,
    ) -> Result<(), UIKitError>
    where
        R: RenderUIKitCustom<Ext>,
    {
        let (view, table_handles) = self.build_block_view(node, provisional)?;
        self.stack_view.addArrangedSubview(&view);
        // Mirror the trailing paragraph spacing that text-like blocks emit via
        // their NSParagraphStyle, so a BlockView followed by another compartment
        // has the same gap as two paragraphs in a TextRun.
        self.stack_view
            .setCustomSpacing_afterView(self.renderer.style().paragraph_spacing, &view);
        let compartment_index = self.compartments.len();
        self.compartments.push(Compartment::BlockView {
            view,
            id,
            table_handles,
            image_urls: Vec::new(),
        });
        self.element_map
            .insert(id, ElementLocation::BlockView { compartment_index });
        Ok(())
    }

    fn build_block_view<Ext>(
        &mut self,
        node: &Node<Ext>,
        provisional: bool,
    ) -> Result<(Retained<UIView>, Option<table::TableHandles>), UIKitError>
    where
        R: RenderUIKitCustom<Ext>,
    {
        let ctx = RenderCtx {
            provisional,
            ..Default::default()
        };
        let style = self.renderer.style();
        let (view, handles): (Retained<UIView>, Option<table::TableHandles>) = match node {
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => {
                let (header_cells, data_rows) = self
                    .renderer
                    .extract_table_data_attributed(header, rows, &ctx);
                let (v, h) =
                    table::build_table_view(&header_cells, &data_rows, alignments, style, self.mtm);
                (v, Some(h))
            }
            Node::CodeBlock { info, literal, .. } => {
                // Skip diagram dispatch on a provisional (mid-stream,
                // unclosed-fence) code block — feeding partial source
                // to the engine just produces repeated syntax errors
                // until the closing fence arrives.
                #[cfg(feature = "pikchr")]
                if !provisional && self.pikchr && info.as_deref() == Some("pikchr") {
                    if let Some(view) = pikchr::try_build_pikchr_view(literal, self.mtm) {
                        return Ok((view, None));
                    }
                }
                #[cfg(feature = "graphviz")]
                if !provisional
                    && self.graphviz
                    && matches!(info.as_deref(), Some("dot") | Some("graphviz"))
                {
                    if let Some(view) = graphviz::try_build_graphviz_view(literal, self.mtm) {
                        return Ok((view, None));
                    }
                }
                #[cfg(feature = "typst")]
                if !provisional && self.typst && info.as_deref() == Some("typst") {
                    if let Some(view) = super::views::typst::try_build_typst_view(literal, self.mtm)
                    {
                        return Ok((view, None));
                    }
                }
                #[cfg(feature = "highlight")]
                let highlighted = info.as_deref().and_then(|lang| {
                    let spans = self.highlighter.as_mut()?.spans(lang, literal)?;
                    let style = self.renderer.style();
                    let font = super::attrs::monospaced_font(style, style.body_font_size);
                    Some(super::syntax::build_highlighted(
                        literal,
                        &spans,
                        style,
                        &font,
                        &style.code_text_color,
                    ))
                });
                #[cfg(not(feature = "highlight"))]
                let highlighted: Option<Retained<NSMutableAttributedString>> = None;
                let style = self.renderer.style();
                (
                    code_block::build_code_block_view(
                        literal,
                        info.as_deref(),
                        highlighted.as_deref().map(|m| &**m),
                        style,
                        self.mtm,
                    ),
                    None,
                )
            }
            Node::BlockQuote { children, .. } => {
                let content = self.renderer.render_blockquote_content(children, &ctx)?;
                (
                    blockquote::build_blockquote_view(&content, style, self.mtm),
                    None,
                )
            }
            Node::ThematicBreak { .. } => (
                thematic_break::build_thematic_break_view(style, self.mtm),
                None,
            ),
            _ => return Err(UIKitError::UnsupportedBlockNode),
        };
        Ok((view, handles))
    }

    fn replace_existing<Ext>(
        &mut self,
        id: CorrelationId,
        loc: ElementLocation,
        node: &Node<Ext>,
        provisional: bool,
    ) -> Result<(), UIKitError>
    where
        R: RenderUIKitCustom<Ext>,
    {
        let new_is_text = is_text_block(node);
        let was_text = matches!(loc, ElementLocation::TextRun { .. });

        // If the kind changed, remove the old and re-insert as new.
        if new_is_text != was_text {
            self.remove_id(id);
            return if new_is_text {
                self.append_text_block(id, node, provisional)
            } else {
                self.append_block_view(id, node, provisional)
            };
        }

        match loc {
            ElementLocation::TextRun { compartment_index } => {
                self.replace_text_block(id, compartment_index, node, provisional)
            }
            ElementLocation::BlockView { compartment_index } => {
                // Fast path: an existing table being replaced by a same-shape
                // table. Mutate the cell text views in place so the parent
                // stack does not invalidate and the surrounding text doesn't
                // jump.
                if let Node::Table { header, rows, .. } = node {
                    if let Compartment::BlockView {
                        table_handles: Some(handles),
                        ..
                    } = &self.compartments[compartment_index]
                    {
                        let ctx = RenderCtx {
                            provisional,
                            ..Default::default()
                        };
                        let (new_header, new_rows) = self
                            .renderer
                            .extract_table_data_attributed(header, rows, &ctx);
                        if handles.shape_matches(&new_header, &new_rows) {
                            handles.update_in_place(&new_header, &new_rows);
                            return Ok(());
                        }
                    }
                }

                let (view, table_handles) = self.build_block_view(node, provisional)?;
                let Compartment::BlockView {
                    view: old_view,
                    id: existing_id,
                    table_handles: old_handles,
                    ..
                } = &mut self.compartments[compartment_index]
                else {
                    return Ok(());
                };
                debug_assert_eq!(*existing_id, id);
                let stack_idx = arranged_index(&self.stack_view, old_view);
                self.stack_view.removeArrangedSubview(old_view);
                old_view.removeFromSuperview();
                if let Some(idx) = stack_idx {
                    self.stack_view.insertArrangedSubview_atIndex(&view, idx);
                } else {
                    self.stack_view.addArrangedSubview(&view);
                }
                self.stack_view
                    .setCustomSpacing_afterView(self.renderer.style().paragraph_spacing, &view);
                *old_view = view;
                *old_handles = table_handles;
                Ok(())
            }
        }
    }

    fn replace_text_block<Ext>(
        &mut self,
        id: CorrelationId,
        compartment_index: usize,
        node: &Node<Ext>,
        provisional: bool,
    ) -> Result<(), UIKitError>
    where
        R: RenderUIKitCustom<Ext>,
    {
        let ctx = RenderCtx {
            provisional,
            ..Default::default()
        };
        let rendered = self.renderer.render_text_block(node, &ctx)?;
        let new_len = rendered.length();

        let Compartment::TextRun {
            content_storage,
            blocks,
            ..
        } = &mut self.compartments[compartment_index]
        else {
            return Ok(());
        };

        let Some(entry_idx) = blocks.iter().position(|e| e.id == id) else {
            return Ok(());
        };
        let old_range = blocks[entry_idx].range;
        let old_len = old_range.length;
        let storage = content_storage
            .textStorage()
            .expect("content storage must have textStorage");

        // Append-only fast path: if the new rendered text starts with the old
        // text in the storage, only insert the tail delta. This is the
        // difference between O(n²) and ~O(n) total work while streaming.
        if new_len > old_len {
            let storage_len = storage.length();
            if old_range.location + old_len <= storage_len {
                let old_text = storage
                    .attributedSubstringFromRange(old_range)
                    .string()
                    .to_string();
                let new_text = rendered.string().to_string();
                if new_text.starts_with(&old_text) {
                    let tail_len = new_len - old_len;
                    let tail =
                        rendered.attributedSubstringFromRange(NSRange::new(old_len, tail_len));
                    let insert_range = NSRange::new(old_range.location + old_len, 0);
                    storage.replaceCharactersInRange_withAttributedString(insert_range, &tail);
                    blocks[entry_idx].range = NSRange::new(old_range.location, new_len);
                    let delta = tail_len as isize;
                    for entry in &mut blocks[entry_idx + 1..] {
                        entry.range.location = (entry.range.location as isize + delta) as usize;
                    }
                    self.register_pending_images(&ctx, compartment_index);
                    return Ok(());
                }
            }
        }

        // Fallback: content diverged, full replace.
        storage.replaceCharactersInRange_withAttributedString(old_range, &rendered);
        blocks[entry_idx].range = NSRange::new(old_range.location, new_len);
        let delta = new_len as isize - old_len as isize;
        for entry in &mut blocks[entry_idx + 1..] {
            entry.range.location = (entry.range.location as isize + delta) as usize;
        }
        self.register_pending_images(&ctx, compartment_index);
        Ok(())
    }

    fn register_pending_images(&mut self, ctx: &RenderCtx, compartment_index: usize) {
        let pending = ctx
            .pending_images
            .borrow_mut()
            .drain(..)
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return;
        }
        match &mut self.compartments[compartment_index] {
            Compartment::TextRun {
                content_storage,
                blocks,
                ..
            } => {
                for p in pending {
                    super::image_loader::load(&p.url, &p.attachment, content_storage, self.mtm);
                    if let Some(last) = blocks.last_mut() {
                        last.image_urls.push(p.url);
                    }
                }
            }
            Compartment::BlockView {
                image_urls, view, ..
            } => {
                // BlockViews don't own a content storage, but we still register
                // the load against a synthetic placeholder content storage held
                // by the view's text views (tables) — for now, just track URLs
                // for cancellation. The image_loader.load call needs a content
                // storage; until BlockView wiring lands, we record only.
                let _ = view;
                for p in pending {
                    image_urls.push(p.url);
                }
            }
        }
    }

    fn apply_delete(&mut self, id: CorrelationId) {
        self.remove_id(id);
    }

    fn remove_id(&mut self, id: CorrelationId) {
        let Some(loc) = self.element_map.swap_remove(&id) else {
            return;
        };
        match loc {
            ElementLocation::TextRun { compartment_index } => {
                let Compartment::TextRun {
                    content_storage,
                    blocks,
                    text_view,
                } = &mut self.compartments[compartment_index]
                else {
                    return;
                };
                let entry_idx = blocks.iter().position(|e| e.id == id);
                let Some(entry_idx) = entry_idx else {
                    return;
                };
                let removed = blocks.remove(entry_idx);
                let storage = content_storage
                    .textStorage()
                    .expect("content storage must have textStorage");
                storage.deleteCharactersInRange(removed.range);
                // Shift subsequent entries.
                for entry in &mut blocks[entry_idx..] {
                    entry.range.location =
                        (entry.range.location as isize - removed.range.length as isize) as usize;
                }
                for url in &removed.image_urls {
                    super::image_loader::cancel(url);
                }

                // If the run is now empty, remove the compartment.
                if blocks.is_empty() {
                    let scroll_view = Retained::into_super(text_view.clone());
                    let view: Retained<UIView> = Retained::into_super(scroll_view);
                    self.stack_view.removeArrangedSubview(&view);
                    view.removeFromSuperview();
                    self.compartments.remove(compartment_index);
                    shift_compartment_indices(&mut self.element_map, compartment_index, -1);
                }
            }
            ElementLocation::BlockView { compartment_index } => {
                let Compartment::BlockView {
                    view, image_urls, ..
                } = &self.compartments[compartment_index]
                else {
                    return;
                };
                self.stack_view.removeArrangedSubview(view);
                view.removeFromSuperview();
                let urls: Vec<String> = image_urls.clone();
                self.compartments.remove(compartment_index);
                shift_compartment_indices(&mut self.element_map, compartment_index, -1);
                for url in &urls {
                    super::image_loader::cancel(url);
                }
            }
        }
    }
}

impl<R, Ext> Sink<Node<Ext>> for BlockSink<R>
where
    R: RenderUIKitCustom<Ext>,
{
    type Error = UIKitError;

    fn emit(&mut self, item: StreamItem<Node<Ext>>) -> Result<(), Self::Error> {
        match item {
            StreamItem::Upsert(emission) => self.apply_upsert(emission),
            StreamItem::Delete { id } => {
                self.apply_delete(id);
                Ok(())
            }
        }
    }
}

fn is_text_block<E>(node: &Node<E>) -> bool {
    matches!(
        node,
        Node::Paragraph { .. }
            | Node::Heading { .. }
            | Node::List { .. }
            | Node::Item { .. }
            | Node::TaskItem { .. }
            | Node::Document { .. }
            | Node::Html { .. }
            | Node::Text { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::MathInline { .. }
            | Node::MathBlock { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::Custom { .. }
    )
}

fn make_stack_view(mtm: MainThreadMarker) -> Retained<UIStackView> {
    let stack = UIStackView::new(mtm);
    stack.setAxis(UILayoutConstraintAxis::Vertical);
    stack
}

fn make_text_run(mtm: MainThreadMarker) -> (Retained<UITextView>, Retained<NSTextContentStorage>) {
    // Build TextKit 2 stack: NSTextContentStorage → NSTextLayoutManager → NSTextContainer → UITextView
    let content_storage = NSTextContentStorage::new();
    let layout_manager = NSTextLayoutManager::new();
    content_storage.addTextLayoutManager(&layout_manager);
    let container = NSTextContainer::new();
    layout_manager.setTextContainer(Some(&container));
    let text_view = UITextView::initWithFrame_textContainer(
        UITextView::alloc(mtm),
        objc2_core_foundation::CGRect::ZERO,
        Some(&container),
    );
    text_view.setEditable(false);
    text_view.setScrollEnabled(false);
    text_view.setBackgroundColor(Some(&UIColor::clearColor()));
    text_view.setTextContainerInset(UIEdgeInsets {
        top: 0.0,
        left: 0.0,
        bottom: 0.0,
        right: 0.0,
    });
    container.setLineFragmentPadding(0.0);
    container.setWidthTracksTextView(true);
    (text_view, content_storage)
}

#[cfg(test)]
mod tests {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::NSTextStorageObserving;

    use crate::node::{Emission, Node, SourceSpan, StreamItem};
    use crate::sink::Sink;

    use super::{BlockSink, Compartment};

    fn test_mtm() -> MainThreadMarker {
        // Same justification as the previous test_mtm in mod.rs: we only manipulate
        // attributed strings and view structs in memory, no actual UIKit display.
        unsafe { MainThreadMarker::new_unchecked() }
    }

    fn paragraph(id: u64, text: &str) -> StreamItem<Node<()>> {
        StreamItem::Upsert(Emission {
            id,
            provisional: false,
            node: Node::Paragraph {
                children: vec![Node::Text {
                    text: text.into(),
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
            },
        })
    }

    fn table(id: u64) -> StreamItem<Node<()>> {
        StreamItem::Upsert(Emission {
            id,
            provisional: false,
            node: Node::Table {
                alignments: vec![None, None],
                header: vec![Node::TableRow {
                    cells: vec![
                        Node::TableCell {
                            children: vec![Node::Text {
                                text: "A".into(),
                                source: SourceSpan::default(),
                            }],
                            header: true,
                            alignment: None,
                            source: SourceSpan::default(),
                        },
                        Node::TableCell {
                            children: vec![Node::Text {
                                text: "B".into(),
                                source: SourceSpan::default(),
                            }],
                            header: true,
                            alignment: None,
                            source: SourceSpan::default(),
                        },
                    ],
                    source: SourceSpan::default(),
                }],
                rows: vec![Node::TableRow {
                    cells: vec![
                        Node::TableCell {
                            children: vec![Node::Text {
                                text: "1".into(),
                                source: SourceSpan::default(),
                            }],
                            header: false,
                            alignment: None,
                            source: SourceSpan::default(),
                        },
                        Node::TableCell {
                            children: vec![Node::Text {
                                text: "2".into(),
                                source: SourceSpan::default(),
                            }],
                            header: false,
                            alignment: None,
                            source: SourceSpan::default(),
                        },
                    ],
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
            },
        })
    }

    fn text_run_text(sink: &BlockSink, idx: usize) -> Option<String> {
        match sink.compartments.get(idx)? {
            Compartment::TextRun {
                content_storage, ..
            } => content_storage
                .textStorage()
                .map(|ts| ts.string().to_string()),
            _ => None,
        }
    }

    fn thematic_break(id: u64) -> StreamItem<Node<()>> {
        StreamItem::Upsert(Emission {
            id,
            provisional: false,
            node: Node::ThematicBreak {
                source: SourceSpan::default(),
            },
        })
    }

    #[test]
    fn thematic_break_creates_block_view_compartment() {
        // ThematicBreak should NOT merge into surrounding text runs — it's its own divider view.
        let mut sink = BlockSink::new(test_mtm());
        sink.emit(paragraph(1, "above")).unwrap();
        sink.emit(thematic_break(2)).unwrap();
        sink.emit(paragraph(3, "below")).unwrap();
        assert_eq!(sink.compartments.len(), 3);
        assert!(matches!(sink.compartments[0], Compartment::TextRun { .. }));
        assert!(matches!(
            sink.compartments[1],
            Compartment::BlockView { .. }
        ));
        assert!(matches!(sink.compartments[2], Compartment::TextRun { .. }));
    }

    #[test]
    fn two_paragraphs_share_one_text_run() {
        let mut sink = BlockSink::new(test_mtm());
        sink.emit(paragraph(1, "alpha")).unwrap();
        sink.emit(paragraph(2, "beta")).unwrap();
        assert_eq!(sink.compartments.len(), 1);
        let text = text_run_text(&sink, 0).unwrap();
        assert!(text.contains("alpha") && text.contains("beta"), "{text:?}");
    }

    #[test]
    fn paragraph_then_table_creates_two_compartments() {
        let mut sink = BlockSink::new(test_mtm());
        sink.emit(paragraph(1, "intro")).unwrap();
        sink.emit(table(2)).unwrap();
        assert_eq!(sink.compartments.len(), 2);
        assert!(matches!(sink.compartments[0], Compartment::TextRun { .. }));
        assert!(matches!(
            sink.compartments[1],
            Compartment::BlockView { .. }
        ));
    }

    #[test]
    fn paragraph_table_paragraph_creates_three_compartments() {
        let mut sink = BlockSink::new(test_mtm());
        sink.emit(paragraph(1, "before")).unwrap();
        sink.emit(table(2)).unwrap();
        sink.emit(paragraph(3, "after")).unwrap();
        assert_eq!(sink.compartments.len(), 3);
        assert!(matches!(sink.compartments[0], Compartment::TextRun { .. }));
        assert!(matches!(
            sink.compartments[1],
            Compartment::BlockView { .. }
        ));
        assert!(matches!(sink.compartments[2], Compartment::TextRun { .. }));
    }

    #[test]
    fn delete_only_block_removes_text_run() {
        let mut sink = BlockSink::new(test_mtm());
        sink.emit(paragraph(1, "lonely")).unwrap();
        assert_eq!(sink.compartments.len(), 1);
        sink.emit(StreamItem::<Node<()>>::Delete { id: 1 }).unwrap();
        assert_eq!(sink.compartments.len(), 0);
    }

    #[test]
    fn delete_one_of_many_keeps_text_run_with_shifted_ranges() {
        let mut sink = BlockSink::new(test_mtm());
        sink.emit(paragraph(1, "aaa")).unwrap();
        sink.emit(paragraph(2, "bbb")).unwrap();
        sink.emit(paragraph(3, "ccc")).unwrap();
        assert_eq!(sink.compartments.len(), 1);
        let before = text_run_text(&sink, 0).unwrap();
        assert!(before.contains("aaa") && before.contains("bbb") && before.contains("ccc"));

        sink.emit(StreamItem::<Node<()>>::Delete { id: 2 }).unwrap();
        assert_eq!(sink.compartments.len(), 1);
        let after = text_run_text(&sink, 0).unwrap();
        assert!(after.contains("aaa") && after.contains("ccc") && !after.contains("bbb"));
    }

    #[test]
    fn streaming_provisional_paragraph_appends_only_tail() {
        // Simulate streaming: a provisional paragraph grows by one token at a time.
        // Verify the final content is correct (which exercises the append fast path).
        let mut sink = BlockSink::new(test_mtm());
        let chunks = [
            "Hello",
            "Hello world",
            "Hello world from",
            "Hello world from markdawn",
        ];
        for text in &chunks {
            sink.emit(StreamItem::Upsert(Emission {
                id: 1,
                provisional: true,
                node: Node::<()>::Paragraph {
                    children: vec![Node::Text {
                        text: (*text).into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();
        }
        let text = text_run_text(&sink, 0).unwrap();
        assert!(text.contains("Hello world from markdawn"), "{text:?}");
    }

    #[test]
    fn upsert_replaces_text_block_in_run() {
        let mut sink = BlockSink::new(test_mtm());
        sink.emit(paragraph(1, "draft")).unwrap();
        sink.emit(paragraph(2, "tail")).unwrap();
        sink.emit(paragraph(1, "final version")).unwrap();
        let text = text_run_text(&sink, 0).unwrap();
        assert!(
            text.contains("final version") && text.contains("tail") && !text.contains("draft"),
            "{text:?}"
        );
    }
}

fn arranged_index(stack: &UIStackView, view: &UIView) -> Option<usize> {
    let arr = stack.arrangedSubviews();
    let count = arr.count();
    for i in 0..count {
        let v = arr.objectAtIndex(i);
        if &*v as *const _ == view as *const _ {
            return Some(i);
        }
    }
    None
}
