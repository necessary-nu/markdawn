mod adapter;
mod editor;
mod error;
mod ext;
#[cfg(feature = "highlight")]
mod highlight;
mod node;
mod parser;
mod sink;

#[cfg(feature = "highlight")]
pub use crate::highlight::SpanHighlighter;
#[cfg(feature = "highlight")]
pub use arborium;

#[cfg(target_os = "ios")]
pub use crate::adapter::{
    BlockSink, RenderCtx, RenderUIKitCustom, TextRenderer, UIKitError, UIKitStyle,
};
pub use crate::adapter::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
pub use crate::editor::EditableDocument;
pub use crate::error::{Error, StreamError};
pub use crate::ext::{BlockMatch, Extension, InlineMatch};
pub use crate::node::{
    AlertKind, CorrelationId, Emission, Node, SourceSpan, StreamItem, TableAlignment,
};
pub use crate::parser::{LiveBlock, Parser, ParserBuilder};
pub use crate::sink::{Sink, VecSink};
