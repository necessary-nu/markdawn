mod adapter;
mod editor;
mod error;
mod ext;
#[cfg(feature = "highlight")]
mod highlight;
mod html_filter;
mod node;
mod parser;
mod serializer;
mod sink;
mod url;

#[cfg(feature = "highlight")]
pub use crate::highlight::SpanHighlighter;
#[cfg(feature = "highlight")]
pub use arborium;

#[cfg(feature = "dioxus")]
pub use crate::adapter::dioxus::{
    DioxusOptions, Markdown, RenderDioxusCustom, Renderer, render_markdown, render_node,
};
#[cfg(target_os = "ios")]
pub use crate::adapter::uikit::cancel_image_load;
#[cfg(target_os = "ios")]
pub use crate::adapter::{
    BlockSink, RenderCtx, RenderUIKitCustom, TextRenderer, UIKitError, UIKitStyle,
};
pub use crate::adapter::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
pub use crate::editor::EditableDocument;
pub use crate::error::{Error, ErrorLocation, StreamError};
pub use crate::ext::{BlockMatch, Extension, InlineMatch};
pub use crate::html_filter::{HtmlFilter, SAFE_DISALLOWED_TAGS};
pub use crate::node::{
    AlertKind, CorrelationId, Emission, Node, SourceSpan, StreamItem, TableAlignment,
};
pub use crate::parser::{LiveBlock, Parser, ParserBuilder};
pub use crate::serializer::{serialize, serialize_blocks};
pub use crate::sink::{Sink, VecSink};
pub use crate::url::{MarkdawnUrl, parse_markdawn_url};
