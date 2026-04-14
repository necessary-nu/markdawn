pub mod html;
#[cfg(target_os = "ios")]
pub mod textkit;

pub use self::html::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
#[cfg(target_os = "ios")]
pub use self::textkit::{
    MarkdownContentStorage, RenderCtx, RenderTextKitCustom, TextKitAdapter, TextKitError,
    TextKitSink, TextKitStyle,
};
