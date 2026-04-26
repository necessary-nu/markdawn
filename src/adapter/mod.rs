#[cfg(feature = "ansi")]
pub mod ansi;
#[cfg(feature = "dioxus")]
pub mod dioxus;
pub mod html;
#[cfg(feature = "iced")]
pub mod iced;
#[cfg(feature = "typst")]
pub mod typst;
#[cfg(target_os = "ios")]
pub mod uikit;

#[cfg(feature = "ansi")]
pub use self::ansi::{
    AnsiAdapter, AnsiError, AnsiOptions, AnsiSink, AnsiWalker, ColorDepth, RenderAnsiCustom,
};
pub use self::html::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
#[cfg(feature = "iced")]
pub use self::iced::{IcedOptions, RenderIcedCustom, Renderer as IcedRenderer, Url as IcedUrl};
#[cfg(feature = "typst")]
pub use self::typst::{RenderTypstCustom, TypstAdapter, TypstError, TypstSink};
#[cfg(target_os = "ios")]
pub use self::uikit::{
    BlockSink, RenderCtx, RenderUIKitCustom, TextRenderer, UIKitError, UIKitStyle,
};
