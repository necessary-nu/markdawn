mod adapter;

/// Reconstructs the umbrella `iced` crate's widget-level surface from the
/// individual iced crates, so the adapter can keep its `iced::` paths without
/// depending on the umbrella `iced` (whose unconditional `iced_winit`
/// dependency drags in winit + a conflicting wasm-bindgen tree). The adapter
/// refers to this as `crate::iced_shim`.
#[cfg(feature = "iced")]
pub(crate) mod iced_shim {
    pub use iced_core::theme::Theme;
    pub use iced_core::*;
    pub use iced_runtime::Task;
    pub use iced_widget as widget;
    pub use iced_widget::Renderer;

    /// `iced_core::Element` has no `Theme`/`Renderer` defaults; the umbrella
    /// `iced::Element` did. Restore them so the adapter's `Element<'a, Message>`
    /// keeps resolving, shadowing the glob-imported `iced_core::Element`.
    pub type Element<'a, Message, Theme = self::Theme, Renderer = self::Renderer> =
        iced_core::Element<'a, Message, Theme, Renderer>;

    /// Mirrors `iced::advanced`.
    pub mod advanced {
        pub use iced_core::renderer::{self, Renderer};
        pub use iced_core::text::Text;
        pub use iced_core::{Shell, layout, overlay, svg, text, widget};
    }
}

#[cfg(feature = "ansi")]
mod ansi_style;
#[cfg(feature = "ansi")]
mod ansi_to_html;
mod editor;
mod error;
mod ext;
mod highlight;
mod html_filter;
mod node;
mod parser;
#[cfg(feature = "pikchr")]
pub mod pikchr_render;
mod serializer;
mod sink;
#[cfg(any(feature = "graphviz", feature = "pikchr", feature = "typst"))]
mod svg_sanitize;
#[cfg(any(feature = "graphviz", feature = "pikchr", feature = "typst"))]
mod svg_text_to_paths;
#[cfg(feature = "terminal")]
mod terminal;
#[cfg(feature = "tracey")]
pub mod tracey;
#[cfg(feature = "typst")]
pub mod typst;
mod url;
mod url_safety;

#[cfg(feature = "ansi")]
pub use crate::ansi_to_html::{ansi_to_annotated_source, ansi_to_html};
#[cfg(feature = "terminal")]
pub use crate::terminal::{
    Cell, TermEmulator, TermGrid, ansi_snapshot_to_ansi, ansi_snapshot_to_html,
};

pub use crate::highlight::{HighlightError, SyntaxHighlighter, SyntaxSpan};
#[cfg(feature = "highlight")]
pub use arborium;

#[cfg(feature = "dioxus")]
pub use crate::adapter::dioxus::{
    DioxusOptions, Markdown, RenderDioxusCustom, Renderer, render_markdown, render_node,
};
#[cfg(feature = "iced")]
pub use crate::adapter::iced::selected_text;
#[cfg(feature = "typst")]
pub use crate::adapter::typst::TypstWalker;
#[cfg(target_os = "ios")]
pub use crate::adapter::uikit::cancel_image_load;
pub use crate::adapter::vdom::{
    AttrVal, Props, RenderVdomCustom, VNode, VdomAdapter, VdomError, VdomSink,
};
#[cfg(feature = "ansi")]
pub use crate::adapter::{
    AnsiAdapter, AnsiError, AnsiOptions, AnsiSink, AnsiWalker, ColorDepth, RenderAnsiCustom,
};
#[cfg(target_os = "ios")]
pub use crate::adapter::{
    BlockSink, RenderCtx, RenderUIKitCustom, TextRenderer, UIKitError, UIKitStyle,
};
pub use crate::adapter::{HtmlAdapter, HtmlError, HtmlSink, RenderHtmlCustom};
#[cfg(feature = "iced")]
pub use crate::adapter::{IcedCodeStyle, IcedOptions, IcedRenderer, IcedUrl, RenderIcedCustom};
#[cfg(feature = "typst")]
pub use crate::adapter::{RenderTypstCustom, TypstAdapter, TypstError, TypstSink};
pub use crate::editor::EditableDocument;
pub use crate::error::{Error, ErrorLocation, StreamError};
pub use crate::ext::{
    BlockMatch, CodeBlockRewrite, CodeBlockRewriter, Extension, InlineContext, InlineMatch,
};
pub use crate::html_filter::HtmlFilter;
pub use crate::node::{
    AlertKind, CorrelationId, Emission, Node, SourceSpan, StreamItem, TableAlignment,
};
pub use crate::parser::{LiveBlock, Parser, ParserBuilder};
pub use crate::serializer::{serialize, serialize_blocks};
pub use crate::sink::{Sink, VecSink};
pub use crate::url::{MarkdawnUrl, parse_markdawn_url};

/// Clear `info` on every `CodeBlock` whose info-language would dispatch
/// to a diagram engine (pikchr / dot / graphviz). Adapters call this
/// on nodes from a *provisional* parse — i.e. an unclosed fenced block
/// mid-stream — to skip the diagram dispatch and fall through to plain
/// rendering. Feeding partial dot/pikchr to its engine just produces
/// repeated syntax errors until the closing fence arrives.
#[allow(dead_code)]
pub(crate) fn clear_provisional_diagram_info<Ext>(node: &mut crate::node::Node<Ext>) {
    node.walk_mut(&mut |n| {
        if is_diagram_code_block(n)
            && let crate::node::Node::CodeBlock { info, .. } = n
        {
            *info = None;
        }
    });
}

/// Same intent as [`clear_provisional_diagram_info`], but for one-shot
/// finalizing parses (where `provisional` is always `false` regardless
/// of fence state). Walks `node`, and for each diagram CodeBlock,
/// inspects its source slice in `source` — if the slice has no closing
/// fence, the block was being typed mid-stream, so strip its info so
/// the renderer falls through to plain text instead of dispatching
/// pikchr/graphviz on every keystroke.
#[allow(dead_code)]
pub(crate) fn clear_unclosed_diagram_info<Ext>(source: &str, node: &mut crate::node::Node<Ext>) {
    node.walk_mut(&mut |n| {
        if !is_diagram_code_block(n) {
            return;
        }
        let span = n.source();
        let slice = source.get(span.start..span.end).unwrap_or("");
        if fenced_code_block_is_closed(slice) {
            return;
        }
        if let crate::node::Node::CodeBlock { info, .. } = n {
            *info = None;
        }
    });
}

#[allow(dead_code)]
fn is_diagram_code_block<Ext>(n: &crate::node::Node<Ext>) -> bool {
    let crate::node::Node::CodeBlock { info, .. } = n else {
        return false;
    };
    let lang = info.as_deref().and_then(|s| s.split_whitespace().next());
    matches!(
        lang,
        Some("dot") | Some("graphviz") | Some("pikchr") | Some("typst")
    )
}

/// `slice` is the source range covering a fenced code block including
/// its opening fence and (if present) its closing fence. Returns true
/// if a matching closing fence is present. Indented (non-fenced) code
/// blocks return true — they can't be "unclosed" in the streaming
/// sense.
#[allow(dead_code)]
fn fenced_code_block_is_closed(slice: &str) -> bool {
    let mut lines = slice.lines();
    let Some(opener) = lines.next() else {
        return true;
    };
    let opener_trimmed = opener.trim_start_matches(' ');
    let marker = match opener_trimmed.chars().next() {
        Some('`') => '`',
        Some('~') => '~',
        _ => return true,
    };
    let opener_count = opener_trimmed.chars().take_while(|&c| c == marker).count();
    if opener_count < 3 {
        return true;
    }
    lines.any(|line| {
        let trimmed = line.trim_start_matches(' ');
        let count = trimmed.chars().take_while(|&c| c == marker).count();
        count >= opener_count && trimmed[count..].chars().all(|c| c.is_whitespace())
    })
}

/// Sanitize + outline a freshly-rendered diagram SVG: drops scripts and
/// disallowed refs, then converts `<text>` to `<path>` against the
/// bundled fonts. `None` if the SVG exceeds the size cap. This is the
/// shared tail of every diagram engine; the per-engine `render_*_to_svg`
/// helpers below and the HTML/vdom adapters all funnel through it.
#[cfg(any(feature = "pikchr", feature = "graphviz", feature = "typst"))]
fn finalize_diagram_svg(svg: String) -> Option<String> {
    if svg.len() > svg_sanitize::MAX_SVG_BYTES {
        return None;
    }
    Some(svg_text_to_paths::text_to_paths(
        &svg_sanitize::sanitize_svg(&svg),
    ))
}

/// Render a `pikchr` diagram source to a sanitized, font-outlined SVG —
/// the exact pipeline used for inline `pikchr` code blocks. `None` on
/// parse failure or oversize output. Exposed so out-of-core renderers
/// (e.g. a lazily-loaded wasm module) produce byte-identical output.
#[cfg(feature = "pikchr")]
pub fn render_pikchr_to_svg(source: &str) -> Option<String> {
    finalize_diagram_svg(pikchr_render::render(source).ok()?)
}

/// Render a `dot`/`graphviz` source to a sanitized, font-outlined SVG.
/// See [`render_pikchr_to_svg`].
#[cfg(feature = "graphviz")]
pub fn render_graphviz_to_svg(source: &str) -> Option<String> {
    finalize_diagram_svg(markdawn_graphviz::render_dot_to_svg(source).ok()?)
}

/// Compile a `typst` source to a sanitized, font-outlined SVG figure.
/// See [`render_pikchr_to_svg`].
#[cfg(feature = "typst")]
pub fn render_typst_to_svg(source: &str) -> Option<String> {
    finalize_diagram_svg(typst::render(source).ok()?)
}
