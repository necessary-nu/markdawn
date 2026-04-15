pub mod html;
#[cfg(target_os = "ios")]
pub mod textkit;

pub use self::html::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
#[cfg(target_os = "ios")]
pub use self::textkit::{
    BlockSink, RenderCtx, RenderTextKitCustom, TextKitError, TextKitStyle, TextRenderer,
};
