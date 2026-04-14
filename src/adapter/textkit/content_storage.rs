use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::NSObjectProtocol;
use objc2_ui_kit::NSTextContentStorage;

define_class!(
    #[unsafe(super(NSTextContentStorage))]
    #[thread_kind = MainThreadOnly]
    #[name = "DarwinMarkdownContentStorage"]
    pub struct MarkdownContentStorage;

    unsafe impl NSObjectProtocol for MarkdownContentStorage {}
);

impl MarkdownContentStorage {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}
