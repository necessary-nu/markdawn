#[cfg(feature = "ansi")]
pub mod ansi;
#[cfg(feature = "dioxus")]
pub mod dioxus;
pub mod html;
#[cfg(target_os = "ios")]
pub mod uikit;

#[cfg(feature = "ansi")]
pub use self::ansi::{
    AnsiAdapter, AnsiError, AnsiOptions, AnsiSink, AnsiWalker, ColorDepth, RenderAnsiCustom,
};
pub use self::html::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
#[cfg(target_os = "ios")]
pub use self::uikit::{
    BlockSink, RenderCtx, RenderUIKitCustom, TextRenderer, UIKitError, UIKitStyle,
};
