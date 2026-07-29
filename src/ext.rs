use crate::node::Node;

// [spec:mw:def:core.ext.match-types]
pub struct InlineMatch<Ext> {
    pub consumed: usize,
    pub node: Node<Ext>,
}

// [spec:mw:def:core.ext.match-types]
pub struct BlockMatch<Ext> {
    pub consumed_lines: usize,
    pub node: Node<Ext>,
}

/// Narrow surface allowing extensions to recursively parse a substring as
/// an inline run. Passed to [`Extension::try_inline_ctx`] so extensions
/// like `:role[<text>](<target>)` can fully parse `<text>` as markdown
/// (emphasis, code, nested roles, etc.) rather than treating it as a
/// literal string.
// [spec:mw:def:core.ext.inline-context]
pub trait InlineContext<Ext> {
    /// Parse `source` as a fresh inline run. Returns the inline nodes.
    /// Reference definitions and footnote labels are NOT inherited from
    /// the surrounding document — the input is treated as self-contained.
    fn parse_inline(&self, source: &str) -> Vec<Node<Ext>>;
}

// [spec:mw:def:core.ext.trait]
pub trait Extension<Ext>: Send + Sync {
    /// Called during inline parsing with the remaining input at the current
    /// position. Return `Some(..)` to consume `consumed` bytes and emit a node.
    fn try_inline(&self, _input: &str) -> Option<InlineMatch<Ext>> {
        None
    }

    /// Context-aware variant of [`Self::try_inline`]. The `ctx` lets the
    /// extension recursively parse captured substrings with full inline
    /// semantics, including other registered extensions. Defaults to
    /// delegating to [`Self::try_inline`] so existing extensions don't
    /// need changes.
    // [spec:mw:req:core.ext.ctx-delegation]
    fn try_inline_ctx(
        &self,
        input: &str,
        _ctx: &dyn InlineContext<Ext>,
    ) -> Option<InlineMatch<Ext>> {
        self.try_inline(input)
    }

    /// Called during block parsing with the document's line buffer and the
    /// current line index. Return `Some(..)` to consume `consumed_lines` lines
    /// starting at `start` and emit a block node.
    fn try_block(&self, _lines: &[String], _start: usize) -> Option<BlockMatch<Ext>> {
        None
    }
}

/// Replacement `(info, literal)` pair emitted by a [`CodeBlockRewriter`].
pub struct CodeBlockRewrite {
    pub info: Option<String>,
    pub literal: String,
}

/// Render-time rewriter for fenced code blocks.
///
/// Called inside `Renderer::render_code_block` before the built-in pikchr /
/// syntax-highlighter / plain-fallback dispatch. A rewriter that returns
/// `Some` replaces the node's `info` + `literal` for rendering purposes
/// only — the parsed node itself is left unchanged, and the rewritten
/// values flow through the rest of the code-block pipeline (pikchr picks
/// up a rewritten `Some("pikchr")` info; the highlighter picks up
/// a rewritten language tag; unknown languages fall through to plain
/// output). Rewriters are tried in registration order; the first one that
/// returns `Some` wins.
///
/// This generalizes pikchr's pattern (hardcoded as `info == Some("pikchr")`
/// inside the renderer) so downstream crates can register their own
/// info-string handlers.
// [spec:mw:def:core.rewriter.types]
pub trait CodeBlockRewriter: Send + Sync {
    fn rewrite(&self, info: Option<&str>, literal: &str) -> Option<CodeBlockRewrite>;
}
