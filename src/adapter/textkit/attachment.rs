use std::sync::Arc;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSAttributedString, NSObjectProtocol};
use objc2_ui_kit::NSTextAttachment;

use super::style::TextKitStyle;

pub enum BlockData {
    Table {
        header_cells: Vec<String>,
        rows: Vec<Vec<String>>,
        col_widths: Vec<usize>,
    },
    CodeBlock {
        literal: String,
        language: Option<String>,
    },
    BlockQuote {
        content: Retained<NSAttributedString>,
    },
}

pub struct AttachmentIvars {
    pub data: BlockData,
    pub style: Arc<TextKitStyle>,
}

define_class!(
    #[unsafe(super(NSTextAttachment))]
    #[name = "MarkdawnBlockAttachment"]
    #[ivars = AttachmentIvars]
    pub struct MarkdownBlockAttachment;

    unsafe impl NSObjectProtocol for MarkdownBlockAttachment {}
);

impl MarkdownBlockAttachment {
    pub fn new_with_data(data: BlockData, style: Arc<TextKitStyle>) -> Retained<Self> {
        let alloc = Self::alloc();
        let partial = alloc.set_ivars(AttachmentIvars { data, style });
        let this: Retained<Self> = unsafe { msg_send![super(partial), init] };
        this.setAllowsTextAttachmentView(true);
        this
    }

    pub fn block_data(&self) -> &BlockData {
        &self.ivars().data
    }

    pub fn block_style(&self) -> &Arc<TextKitStyle> {
        &self.ivars().style
    }
}
