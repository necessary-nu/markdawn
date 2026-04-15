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
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSMutableAttributedString, NSRange};
use objc2_ui_kit::{
    NSLayoutConstraintAxis, NSTextContainer, NSTextContentManager, NSTextContentStorage,
    NSTextLayoutManager, NSTextStorageObserving, UIStackView, UITextView, UIView,
};

use crate::node::{CorrelationId, Emission, Node, StreamItem};
use crate::sink::Sink;

use super::element_map::{shift_compartment_indices, ElementLocation, ElementMap};
use super::render::{RenderCtx, RenderTextKitCustom, TextKitError, TextRenderer};
use super::style::TextKitStyle;
use super::views::{blockquote, code_block, table};

pub struct BlockSink<R = ()> {
    renderer: TextRenderer<R>,
    stack_view: Retained<UIStackView>,
    compartments: Vec<Compartment>,
    element_map: ElementMap,
    available_width: f64,
    mtm: MainThreadMarker,
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
    },
}

struct TextBlockEntry {
    id: CorrelationId,
    range: NSRange,
}

impl BlockSink<()> {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self::with_arc_style(mtm, Arc::new(TextKitStyle::default()))
    }

    pub fn with_style(mtm: MainThreadMarker, style: TextKitStyle) -> Self {
        Self::with_arc_style(mtm, Arc::new(style))
    }

    fn with_arc_style(mtm: MainThreadMarker, style: Arc<TextKitStyle>) -> Self {
        let stack_view = make_stack_view(mtm);
        Self {
            renderer: TextRenderer::new(style),
            stack_view,
            compartments: Vec::new(),
            element_map: ElementMap::new(),
            available_width: 320.0,
            mtm,
        }
    }
}

impl<R> BlockSink<R> {
    pub fn with_custom_renderer(
        mtm: MainThreadMarker,
        style: TextKitStyle,
        custom: R,
    ) -> Self {
        let stack_view = make_stack_view(mtm);
        Self {
            renderer: TextRenderer::with_custom(Arc::new(style), custom),
            stack_view,
            compartments: Vec::new(),
            element_map: ElementMap::new(),
            available_width: 320.0,
            mtm,
        }
    }

    /// The `UIStackView` that arranges all rendered blocks. Embed this in your hierarchy.
    pub fn stack_view(&self) -> &UIStackView {
        &self.stack_view
    }

    /// Set the layout width used when building block views (tables, code blocks, etc).
    /// Existing block views are NOT re-laid out — call this before rendering or accept
    /// some misalignment until the next view is created.
    pub fn set_available_width(&mut self, width: f64) {
        self.available_width = width;
    }

    fn apply_upsert<Ext, Meta>(
        &mut self,
        emission: Emission<Node<Ext, Meta>>,
    ) -> Result<(), TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
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

    fn append_text_block<Ext, Meta>(
        &mut self,
        id: CorrelationId,
        node: &Node<Ext, Meta>,
        provisional: bool,
    ) -> Result<(), TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
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

        if append_to_last {
            let compartment_index = last_idx.unwrap();
            let Compartment::TextRun {
                content_storage,
                blocks,
                ..
            } = &mut self.compartments[compartment_index]
            else {
                unreachable!()
            };
            let storage = content_storage
                .textStorage()
                .expect("content storage must have textStorage");
            let insert_loc = unsafe { storage.length() };
            unsafe { storage.appendAttributedString(&rendered) };
            blocks.push(TextBlockEntry {
                id,
                range: NSRange::new(insert_loc, len),
            });
            self.element_map
                .insert(id, ElementLocation::TextRun { compartment_index });
        } else {
            // Create a new TextRun compartment.
            let (text_view, content_storage) = make_text_run(self.mtm);
            let storage = content_storage
                .textStorage()
                .expect("content storage must have textStorage");
            unsafe { storage.appendAttributedString(&rendered) };

            self.stack_view.addArrangedSubview(&text_view);
            let compartment_index = self.compartments.len();
            self.compartments.push(Compartment::TextRun {
                text_view,
                content_storage,
                blocks: vec![TextBlockEntry {
                    id,
                    range: NSRange::new(0, len),
                }],
            });
            self.element_map
                .insert(id, ElementLocation::TextRun { compartment_index });
        }

        Ok(())
    }

    fn append_block_view<Ext, Meta>(
        &mut self,
        id: CorrelationId,
        node: &Node<Ext, Meta>,
        provisional: bool,
    ) -> Result<(), TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let view = self.build_block_view(node, provisional)?;
        self.stack_view.addArrangedSubview(&view);
        let compartment_index = self.compartments.len();
        self.compartments.push(Compartment::BlockView { view, id });
        self.element_map
            .insert(id, ElementLocation::BlockView { compartment_index });
        Ok(())
    }

    fn build_block_view<Ext, Meta>(
        &self,
        node: &Node<Ext, Meta>,
        provisional: bool,
    ) -> Result<Retained<UIView>, TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let ctx = RenderCtx {
            provisional,
            ..Default::default()
        };
        let style = self.renderer.style();
        let view: Retained<UIView> = match node {
            Node::Table { header, rows, .. } => {
                let (header_cells, data_rows, col_widths) =
                    self.renderer.extract_table_data(header, rows, &ctx);
                table::build_table_view(
                    &header_cells,
                    &data_rows,
                    &col_widths,
                    style,
                    self.available_width,
                    self.mtm,
                )
            }
            Node::CodeBlock { info, literal, .. } => code_block::build_code_block_view(
                literal,
                info.as_deref(),
                style,
                self.available_width,
                self.mtm,
            ),
            Node::BlockQuote { children, .. } => {
                let content = self.renderer.render_blockquote_content(children, &ctx)?;
                blockquote::build_blockquote_view(
                    &content,
                    style,
                    self.available_width,
                    self.mtm,
                )
            }
            _ => return Err(TextKitError::UnsupportedBlockNode),
        };
        Ok(view)
    }

    fn replace_existing<Ext, Meta>(
        &mut self,
        id: CorrelationId,
        loc: ElementLocation,
        node: &Node<Ext, Meta>,
        provisional: bool,
    ) -> Result<(), TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
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
                let view = self.build_block_view(node, provisional)?;
                let Compartment::BlockView {
                    view: old_view,
                    id: existing_id,
                } = &mut self.compartments[compartment_index]
                else {
                    return Ok(());
                };
                debug_assert_eq!(*existing_id, id);
                let stack_idx = arranged_index(&self.stack_view, old_view);
                self.stack_view.removeArrangedSubview(old_view);
                old_view.removeFromSuperview();
                if let Some(idx) = stack_idx {
                    unsafe {
                        self.stack_view.insertArrangedSubview_atIndex(&view, idx);
                    }
                } else {
                    self.stack_view.addArrangedSubview(&view);
                }
                *old_view = view;
                Ok(())
            }
        }
    }

    fn replace_text_block<Ext, Meta>(
        &mut self,
        id: CorrelationId,
        compartment_index: usize,
        node: &Node<Ext, Meta>,
        provisional: bool,
    ) -> Result<(), TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
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

        let entry_idx = blocks.iter().position(|e| e.id == id);
        let Some(entry_idx) = entry_idx else {
            return Ok(());
        };
        let old_range = blocks[entry_idx].range;
        let storage = content_storage
            .textStorage()
            .expect("content storage must have textStorage");

        unsafe {
            storage.replaceCharactersInRange_withAttributedString(old_range, &rendered);
        }

        // Update this entry's range and shift subsequent ones.
        blocks[entry_idx].range = NSRange::new(old_range.location, new_len);
        let delta = new_len as isize - old_range.length as isize;
        for entry in &mut blocks[entry_idx + 1..] {
            entry.range.location = (entry.range.location as isize + delta) as usize;
        }
        Ok(())
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
                unsafe {
                    storage.deleteCharactersInRange(removed.range);
                }
                // Shift subsequent entries.
                for entry in &mut blocks[entry_idx..] {
                    entry.range.location = (entry.range.location as isize
                        - removed.range.length as isize)
                        as usize;
                }

                // If the run is now empty, remove the compartment.
                if blocks.is_empty() {
                    let view: Retained<UIView> = text_view.clone().into_super();
                    self.stack_view.removeArrangedSubview(&view);
                    view.removeFromSuperview();
                    self.compartments.remove(compartment_index);
                    shift_compartment_indices(
                        &mut self.element_map,
                        compartment_index,
                        -1,
                    );
                }
            }
            ElementLocation::BlockView { compartment_index } => {
                let Compartment::BlockView { view, .. } =
                    &self.compartments[compartment_index]
                else {
                    return;
                };
                self.stack_view.removeArrangedSubview(view);
                view.removeFromSuperview();
                self.compartments.remove(compartment_index);
                shift_compartment_indices(&mut self.element_map, compartment_index, -1);
            }
        }
    }
}

impl<R, Ext, Meta> Sink<Node<Ext, Meta>> for BlockSink<R>
where
    R: RenderTextKitCustom<Ext, Meta>,
{
    type Error = TextKitError;

    fn emit(&mut self, item: StreamItem<Node<Ext, Meta>>) -> Result<(), Self::Error> {
        match item {
            StreamItem::Upsert(emission) => self.apply_upsert(emission),
            StreamItem::Delete { id } => {
                self.apply_delete(id);
                Ok(())
            }
        }
    }
}

fn is_text_block<E, M>(node: &Node<E, M>) -> bool {
    matches!(
        node,
        Node::Paragraph { .. }
            | Node::Heading { .. }
            | Node::List { .. }
            | Node::Item { .. }
            | Node::TaskItem { .. }
            | Node::ThematicBreak { .. }
            | Node::Document { .. }
            | Node::Html { .. }
            | Node::Text { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::Custom { .. }
    )
}

fn make_stack_view(mtm: MainThreadMarker) -> Retained<UIStackView> {
    let stack = UIStackView::new(mtm);
    unsafe {
        stack.setAxis(NSLayoutConstraintAxis::Vertical);
    }
    stack
}

fn make_text_run(mtm: MainThreadMarker) -> (Retained<UITextView>, Retained<NSTextContentStorage>) {
    // Build TextKit 2 stack: NSTextContentStorage → NSTextLayoutManager → NSTextContainer → UITextView
    let content_storage = NSTextContentStorage::new();
    let layout_manager = NSTextLayoutManager::new(mtm);
    unsafe {
        content_storage.addTextLayoutManager(&layout_manager);
    }
    let container = NSTextContainer::new();
    unsafe {
        layout_manager.setTextContainer(Some(&container));
    }
    let text_view = unsafe {
        UITextView::initWithFrame_textContainer(
            UITextView::alloc(mtm),
            objc2_core_foundation::CGRect::ZERO,
            Some(&container),
        )
    };
    text_view.setEditable(false);
    text_view.setScrollEnabled(false);
    (text_view, content_storage)
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
