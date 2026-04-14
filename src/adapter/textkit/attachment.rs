use std::sync::Arc;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, AnyThread, ClassType, DefinedClass};
use objc2_foundation::{ns_string, NSAttributedString, NSObjectProtocol, NSString};
use objc2_ui_kit::NSTextAttachment;

use super::style::TextKitStyle;
use super::views::MarkdownAttachmentViewProvider;

/// File type used to register our view provider class with TextKit.
pub(crate) fn block_file_type() -> &'static NSString {
    ns_string!("dev.markdawn.block")
}

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
        ensure_view_provider_registered();

        let alloc = Self::alloc();
        let partial = alloc.set_ivars(AttachmentIvars { data, style });
        let this: Retained<Self> = unsafe { msg_send![super(partial), init] };
        this.setAllowsTextAttachmentView(true);
        this.setFileType(Some(block_file_type()));
        this
    }

    pub fn block_data(&self) -> &BlockData {
        &self.ivars().data
    }

    pub fn block_style(&self) -> &Arc<TextKitStyle> {
        &self.ivars().style
    }
}

/// Register our view provider class with TextKit, once per process.
fn ensure_view_provider_registered() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        NSTextAttachment::registerTextAttachmentViewProviderClass_forFileType(
            MarkdownAttachmentViewProvider::class(),
            block_file_type(),
        );
    });
}
