mod adapter;
#[cfg(feature = "ansi")]
mod ansi_style;
#[cfg(feature = "ansi")]
mod ansi_to_html;
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
#[cfg(any(feature = "graphviz", feature = "pikchr"))]
mod svg_sanitize;
#[cfg(any(feature = "graphviz", feature = "pikchr"))]
mod svg_text_to_paths;
#[cfg(feature = "pikchr")]
mod pikchr_render;
mod url_safety;
#[cfg(feature = "terminal")]
mod terminal;
mod url;

#[cfg(feature = "ansi")]
pub use crate::ansi_to_html::{ansi_to_annotated_source, ansi_to_html};
#[cfg(feature = "terminal")]
pub use crate::terminal::{
    Cell, TermEmulator, TermGrid, ansi_snapshot_to_ansi, ansi_snapshot_to_html,
};

#[cfg(feature = "highlight")]
pub use crate::highlight::SpanHighlighter;
#[cfg(feature = "highlight")]
pub use arborium;

#[cfg(feature = "dioxus")]
pub use crate::adapter::dioxus::{
    DioxusOptions, Markdown, RenderDioxusCustom, Renderer, render_markdown, render_node,
};
#[cfg(feature = "typst")]
pub use crate::adapter::typst::TypstWalker;
#[cfg(target_os = "ios")]
pub use crate::adapter::uikit::cancel_image_load;
#[cfg(feature = "ansi")]
pub use crate::adapter::{
    AnsiAdapter, AnsiError, AnsiOptions, AnsiSink, AnsiWalker, ColorDepth, RenderAnsiCustom,
};
#[cfg(target_os = "ios")]
pub use crate::adapter::{
    BlockSink, RenderCtx, RenderUIKitCustom, TextRenderer, UIKitError, UIKitStyle,
};
pub use crate::adapter::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
#[cfg(feature = "typst")]
pub use crate::adapter::{RenderTypstCustom, TypstAdapter, TypstError, TypstSink};
pub use crate::editor::EditableDocument;
pub use crate::error::{Error, ErrorLocation, StreamError};
pub use crate::ext::{
    BlockMatch, CodeBlockRewrite, CodeBlockRewriter, Extension, InlineContext, InlineMatch,
};
pub use crate::html_filter::{HtmlFilter, SAFE_DISALLOWED_TAGS};
pub use crate::node::{
    AlertKind, CorrelationId, Emission, Node, SourceSpan, StreamItem, TableAlignment,
};
pub use crate::parser::{LiveBlock, Parser, ParserBuilder};
pub use crate::serializer::{serialize, serialize_blocks};
pub use crate::sink::{Sink, VecSink};
pub use crate::url::{MarkdawnUrl, parse_markdawn_url};
