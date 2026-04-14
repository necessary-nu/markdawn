pub(crate) mod blockquote;
pub(crate) mod code_block;
pub(crate) mod table;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSAttributedStringKey, NSDictionary, NSObjectProtocol};
use objc2_ui_kit::{
    NSTextAttachmentViewProvider, NSTextContainer, NSTextLayoutManager, NSTextLocation, UIView,
};

use super::attachment::{BlockData, MarkdownBlockAttachment};

define_class!(
    #[unsafe(super(NSTextAttachmentViewProvider))]
    #[thread_kind = MainThreadOnly]
    #[name = "MarkdawnAttachmentViewProvider"]
    pub struct MarkdownAttachmentViewProvider;

    unsafe impl NSObjectProtocol for MarkdownAttachmentViewProvider {}

    impl MarkdownAttachmentViewProvider {
        #[unsafe(method(loadView))]
        fn __load_view(&self) {
            self.do_load_view();
        }

        #[unsafe(method(attachmentBoundsForAttributes:location:textContainer:proposedLineFragment:position:))]
        fn __attachment_bounds(
            &self,
            _attributes: &NSDictionary<NSAttributedStringKey, AnyObject>,
            _location: &ProtocolObject<dyn NSTextLocation>,
            _text_container: Option<&NSTextContainer>,
            proposed_line_fragment: CGRect,
            _position: CGPoint,
        ) -> CGRect {
            self.do_compute_bounds(proposed_line_fragment)
        }
    }
);

impl MarkdownAttachmentViewProvider {
    pub fn new_with(
        attachment: &MarkdownBlockAttachment,
        parent_view: Option<&UIView>,
        layout_manager: Option<&NSTextLayoutManager>,
        location: &ProtocolObject<dyn NSTextLocation>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        unsafe {
            msg_send![
                Self::alloc(mtm),
                initWithTextAttachment: attachment,
                parentView: parent_view,
                textLayoutManager: layout_manager,
                location: location,
            ]
        }
    }

    fn do_load_view(&self) {
        let mtm = MainThreadMarker::from(self);

        let Some(attachment) = self.textAttachment() else {
            return;
        };
        let Some(attachment) = attachment.downcast_ref::<MarkdownBlockAttachment>() else {
            return;
        };

        // Get the available width from the text layout manager's primary container,
        // falling back to a sensible default.
        let proposed_width = self
            .textLayoutManager()
            .and_then(|lm| unsafe { lm.textContainer() })
            .map(|tc| unsafe { tc.size() }.width)
            .filter(|w| *w > 0.0)
            .unwrap_or(320.0);

        let style = attachment.block_style();
        let data = attachment.block_data();

        let view = match data {
            BlockData::Table {
                header_cells,
                rows,
                col_widths,
            } => table::build_table_view(
                header_cells,
                rows,
                col_widths,
                style,
                proposed_width,
                mtm,
            ),
            BlockData::CodeBlock { literal, language } => code_block::build_code_block_view(
                literal,
                language.as_deref(),
                style,
                proposed_width,
                mtm,
            ),
            BlockData::BlockQuote { content } => {
                blockquote::build_blockquote_view(content, style, proposed_width, mtm)
            }
        };

        self.setView(Some(&view));
        self.setTracksTextAttachmentViewBounds(true);
    }

    fn do_compute_bounds(&self, proposed_line_fragment: CGRect) -> CGRect {
        let width = proposed_line_fragment.size.width;

        let Some(attachment) = self.textAttachment() else {
            return CGRect::ZERO;
        };
        let Some(attachment) = attachment.downcast_ref::<MarkdownBlockAttachment>() else {
            return CGRect::ZERO;
        };

        let style = attachment.block_style();
        let data = attachment.block_data();

        let height = match data {
            BlockData::Table { rows, .. } => table::compute_table_height(rows.len(), style),
            BlockData::CodeBlock { literal, .. } => {
                code_block::compute_code_block_height(literal, style)
            }
            BlockData::BlockQuote { content } => {
                blockquote::compute_blockquote_height(content, style, width)
            }
        };

        CGRect::new(CGPoint::ZERO, CGSize { width, height })
    }
}
