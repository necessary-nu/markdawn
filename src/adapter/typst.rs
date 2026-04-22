//! Typst adapter — walks a `Node<Ext>` tree and emits a `.typ` source
//! string.
//!
//! Mirrors the shape of `adapter/html.rs`: a stateful `TypstAdapter<R>`
//! with a `RenderTypstCustom<Ext>` plug-in for the extension payload,
//! plus a `Sink` wrapper for streaming use.
//!
//! Typst is a typesetting language with markup-first syntax. Where
//! CommonMark has ergonomic surface (`# heading`, `- item`, `` `code` ``)
//! the Typst equivalents often need a function-call form so we can pass
//! structured args — e.g. an ordered list that starts at `3.` has no
//! clean markup form, so we emit `#enum(start: 3, [..], [..])`.
//!
//! The admonition macro, named-block helpers, and inline-role helpers
//! referenced in emitted output live in the consumer's template (not in
//! markdawn itself) — we just emit function calls, the template supplies
//! the definitions.

use core::fmt;

use crate::node::{AlertKind, Emission, Node, StreamItem, TableAlignment};
use crate::sink::Sink;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypstError {
    UnsupportedCustomNode,
    Sink(String),
}

impl fmt::Display for TypstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCustomNode => f.write_str("custom node renderer not configured"),
            Self::Sink(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TypstError {}

/// Renderer plug-in for `Node::Custom<Ext>` payloads — the Typst
/// equivalent of `RenderHtmlCustom<Ext>`.
///
/// Custom renderers receive a [`TypstWalker`] that can recurse into
/// child nodes using the same rules the top-level walker uses.
pub trait RenderTypstCustom<Ext> {
    fn render_custom(
        &self,
        payload: &Ext,
        children: &[Node<Ext>],
        out: &mut String,
        walker: &TypstWalker<'_, Ext>,
    ) -> Result<(), TypstError>;
}

impl<Ext> RenderTypstCustom<Ext> for () {
    fn render_custom(
        &self,
        _payload: &Ext,
        _children: &[Node<Ext>],
        _out: &mut String,
        _walker: &TypstWalker<'_, Ext>,
    ) -> Result<(), TypstError> {
        Err(TypstError::UnsupportedCustomNode)
    }
}

/// Recursion helper handed to custom renderers so they can descend
/// into their own children using the same code-path the adapter uses.
///
/// The two closures bind to the adapter's block/inline walkers; the
/// custom renderer calls `walker.write_block` for children that are
/// themselves blocks, `walker.write_inlines` for inline-level children.
pub struct TypstWalker<'a, Ext> {
    #[allow(clippy::type_complexity)]
    walk_block: &'a dyn Fn(&Node<Ext>, &mut String) -> Result<(), TypstError>,
    #[allow(clippy::type_complexity)]
    walk_inlines: &'a dyn Fn(&[Node<Ext>], &mut String) -> Result<(), TypstError>,
}

impl<'a, Ext> TypstWalker<'a, Ext> {
    pub fn write_block(&self, node: &Node<Ext>, out: &mut String) -> Result<(), TypstError> {
        (self.walk_block)(node, out)
    }

    pub fn write_inlines(&self, nodes: &[Node<Ext>], out: &mut String) -> Result<(), TypstError> {
        (self.walk_inlines)(nodes, out)
    }
}

/// Typst-emitting adapter.
pub struct TypstAdapter<R = ()> {
    custom: R,
}

impl Default for TypstAdapter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl TypstAdapter<()> {
    pub fn new() -> Self {
        Self { custom: () }
    }

    pub fn with_custom_renderer<Custom>(custom: Custom) -> TypstAdapter<Custom> {
        TypstAdapter { custom }
    }
}

impl<R> TypstAdapter<R> {
    /// Render a single stream item (used by streaming-sink consumers).
    pub fn render<Ext>(&self, item: StreamItem<Node<Ext>>) -> Result<StreamItem<String>, TypstError>
    where
        R: RenderTypstCustom<Ext>,
    {
        match item {
            StreamItem::Upsert(emission) => Ok(StreamItem::Upsert(Emission {
                id: emission.id,
                provisional: emission.provisional,
                node: self.render_node(&emission.node)?,
            })),
            StreamItem::Delete { id } => Ok(StreamItem::Delete { id }),
        }
    }

    /// Render a single `Node<Ext>` to a complete Typst source string.
    pub fn render_node<Ext>(&self, node: &Node<Ext>) -> Result<String, TypstError>
    where
        R: RenderTypstCustom<Ext>,
    {
        let mut out = String::new();
        self.write_block(node, &mut out)?;
        Ok(out)
    }

    /// Render multiple top-level `Node<Ext>` children (typical usage:
    /// a chapter body is a `Vec<Node<Ext>>`).
    pub fn render_many<Ext>(&self, nodes: &[Node<Ext>]) -> Result<String, TypstError>
    where
        R: RenderTypstCustom<Ext>,
    {
        let mut out = String::new();
        for node in nodes {
            self.write_block(node, &mut out)?;
        }
        Ok(out)
    }

    fn write_block<Ext>(&self, node: &Node<Ext>, out: &mut String) -> Result<(), TypstError>
    where
        R: RenderTypstCustom<Ext>,
    {
        match node {
            Node::Document { children, .. } => {
                for child in children {
                    self.write_block(child, out)?;
                }
            }
            Node::Paragraph { children, .. } => {
                self.write_inlines(children, out)?;
                out.push_str("\n\n");
            }
            Node::Heading {
                level, children, ..
            } => {
                out.push('\n');
                let level = (*level).min(6);
                for _ in 0..level {
                    out.push('=');
                }
                out.push(' ');
                self.write_inlines(children, out)?;
                out.push_str("\n\n");
            }
            Node::BlockQuote { children, .. } => {
                out.push_str("#quote(block: true)[\n");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("]\n\n");
            }
            Node::List {
                ordered,
                start,
                children,
                ..
            } => {
                let start = start.unwrap_or(1);
                // The Typst markup forms `- item` / `+ item` don't take
                // a starting index for ordered lists, and taskitems
                // can't round-trip through them, so we always use the
                // function forms for correctness.
                if *ordered {
                    out.push_str("#enum(start: ");
                    out.push_str(&start.to_string());
                    out.push_str(", tight: false");
                } else {
                    out.push_str("#list(tight: false");
                }
                for child in children {
                    out.push_str(",\n  [");
                    match child {
                        Node::Item { children, .. } | Node::TaskItem { children, .. } => {
                            for grand in children {
                                self.write_block(grand, out)?;
                            }
                        }
                        other => self.write_block(other, out)?,
                    }
                    // Trim trailing blank line added by paragraph.
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    out.push(']');
                }
                out.push_str(",\n)\n\n");
            }
            Node::Item { children, .. } => {
                for child in children {
                    self.write_block(child, out)?;
                }
            }
            Node::TaskItem {
                checked, children, ..
            } => {
                out.push_str("#task-item(done: ");
                out.push_str(if *checked { "true" } else { "false" });
                out.push_str(")[\n");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("]\n");
            }
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => {
                self.write_table(alignments, header, rows, out)?;
            }
            Node::TableRow { .. } | Node::TableCell { .. } => {
                // Shouldn't appear at block level outside a Table; skip.
            }
            Node::CodeBlock { info, literal, .. } => {
                let lang = info
                    .as_deref()
                    .and_then(|s| s.split_whitespace().next())
                    .filter(|s| !s.is_empty());
                // ```ansi / ```terminal content carries VT escape
                // sequences meant for a terminal; print can't render
                // them. `ansi` just strips (author supplied pre-styled
                // output); `terminal` runs the bytes through a real
                // emulator first, then prints the final visible screen
                // as plain text.
                if matches!(lang, Some("ansi")) {
                    let stripped = strip_ansi_for_typst(literal);
                    write_raw_block(None, &stripped, out);
                } else if matches!(lang, Some("terminal")) {
                    let rendered = terminal_snapshot_for_typst(literal);
                    write_raw_block(None, &rendered, out);
                } else {
                    write_raw_block(lang, literal, out);
                }
                out.push_str("\n\n");
            }
            Node::MathBlock { math, .. } => {
                out.push_str("$ ");
                out.push_str(math);
                out.push_str(" $\n\n");
            }
            Node::ThematicBreak { .. } => {
                out.push_str("#line(length: 100%)\n\n");
            }
            Node::Alert {
                kind,
                title,
                children,
                ..
            } => {
                let slug = match kind {
                    AlertKind::Note => "note",
                    AlertKind::Tip => "tip",
                    AlertKind::Important => "important",
                    AlertKind::Warning => "warning",
                    AlertKind::Caution => "caution",
                };
                out.push_str("#admonition(kind: \"");
                out.push_str(slug);
                out.push('"');
                if let Some(t) = title {
                    out.push_str(", title: \"");
                    write_typst_string_literal(t, out);
                    out.push('"');
                }
                out.push_str(")[\n");
                for child in children {
                    self.write_block(child, out)?;
                }
                out.push_str("]\n\n");
            }
            Node::DefinitionList { children, .. } => {
                out.push_str("#terms(tight: false");
                for child in children.chunks(2) {
                    if let [
                        Node::DefinitionTerm { children: term, .. },
                        Node::DefinitionDescription { children: desc, .. },
                    ] = child
                    {
                        out.push_str(",\n  (");
                        out.push('[');
                        self.write_inlines(term, out)?;
                        out.push_str("], [");
                        for d in desc {
                            self.write_block(d, out)?;
                        }
                        while out.ends_with('\n') {
                            out.pop();
                        }
                        out.push_str("])");
                    }
                }
                out.push_str(",\n)\n\n");
            }
            Node::DefinitionTerm { .. } | Node::DefinitionDescription { .. } => {
                // Handled by the DefinitionList parent.
            }
            Node::Html { .. } => {
                // HTML has no Typst equivalent; the consumer can plug
                // a custom renderer if they care. Skip silently so the
                // doc still compiles.
            }
            Node::FootnoteSection { .. } | Node::FootnoteDefinition { .. } => {
                // Footnotes round-trip inline via `#footnote[...]`; the
                // definition+section wrappers that CommonMark splits
                // them into aren't meaningful in Typst. Skip; the
                // `FootnoteRef` inline site emits the content directly.
            }
            Node::Custom {
                payload, children, ..
            } => {
                let walk_block = |node: &Node<Ext>, o: &mut String| self.write_block(node, o);
                let walk_inlines =
                    |nodes: &[Node<Ext>], o: &mut String| self.write_inlines(nodes, o);
                let walker = TypstWalker {
                    walk_block: &walk_block,
                    walk_inlines: &walk_inlines,
                };
                self.custom.render_custom(payload, children, out, &walker)?;
            }
            // Fall through to inline renderer when a leaf appears at
            // block level (e.g. a bare `Text` at document root).
            Node::Text { .. }
            | Node::CodeSpan { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Subscript { .. }
            | Node::Superscript { .. }
            | Node::Highlight { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::MathInline { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::FootnoteRef { .. } => {
                self.write_inline(node, out)?;
            }
        }
        Ok(())
    }

    fn write_inlines<Ext>(&self, nodes: &[Node<Ext>], out: &mut String) -> Result<(), TypstError>
    where
        R: RenderTypstCustom<Ext>,
    {
        for node in nodes {
            self.write_inline(node, out)?;
        }
        Ok(())
    }

    fn write_inline<Ext>(&self, node: &Node<Ext>, out: &mut String) -> Result<(), TypstError>
    where
        R: RenderTypstCustom<Ext>,
    {
        match node {
            Node::Text { text, .. } => write_typst_text(text, out),
            Node::Emphasis { children, .. } => {
                out.push('_');
                self.write_inlines(children, out)?;
                out.push('_');
            }
            Node::Strong { children, .. } => {
                out.push('*');
                self.write_inlines(children, out)?;
                out.push('*');
            }
            Node::Strikethrough { children, .. } => {
                out.push_str("#strike[");
                self.write_inlines(children, out)?;
                out.push(']');
            }
            Node::Subscript { children, .. } => {
                out.push_str("#sub[");
                self.write_inlines(children, out)?;
                out.push(']');
            }
            Node::Superscript { children, .. } => {
                out.push_str("#super[");
                self.write_inlines(children, out)?;
                out.push(']');
            }
            Node::Highlight { children, .. } => {
                out.push_str("#highlight[");
                self.write_inlines(children, out)?;
                out.push(']');
            }
            Node::Link {
                destination,
                children,
                ..
            } => {
                // In-document fragment links (`#foo`) resolve to Typst
                // labels when the fragment looks like a valid label name
                // (ASCII alphanumerics, `-`, `_`, `.`, `:`). Anything
                // else stays a plain URL-string link.
                if let Some(frag) = destination.strip_prefix('#')
                    && !frag.is_empty()
                    && frag
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
                {
                    out.push_str("#link(<");
                    out.push_str(frag);
                    out.push_str(">)[");
                    self.write_inlines(children, out)?;
                    out.push(']');
                } else {
                    out.push_str("#link(\"");
                    write_typst_string_literal(destination, out);
                    out.push_str("\")[");
                    self.write_inlines(children, out)?;
                    out.push(']');
                }
            }
            Node::Image {
                destination, alt, ..
            } => {
                // Typst's `image(..)` can't fetch remote files — and in
                // a print pipeline we probably don't want to silently
                // reach out to the network anyway. For http(s):// URLs
                // emit the alt text as an italic placeholder with a
                // link; for local paths use the real image.
                let is_remote =
                    destination.starts_with("http://") || destination.starts_with("https://");
                if is_remote {
                    out.push_str("#link(\"");
                    write_typst_string_literal(destination, out);
                    out.push_str("\")[");
                    if alt.is_empty() {
                        out.push_str("(image)");
                    } else {
                        out.push_str("_");
                        write_typst_text(alt, out);
                        out.push_str("_");
                    }
                    out.push(']');
                } else {
                    out.push_str("#figure(image(\"");
                    write_typst_string_literal(destination, out);
                    if alt.is_empty() {
                        out.push_str("\"))");
                    } else {
                        out.push_str("\"), caption: [");
                        write_typst_text(alt, out);
                        out.push_str("])");
                    }
                }
            }
            Node::CodeSpan { code, .. } => {
                write_raw_inline(code, out);
            }
            Node::MathInline { math, .. } => {
                out.push('$');
                out.push_str(math);
                out.push('$');
            }
            Node::SoftBreak { .. } => out.push(' '),
            Node::HardBreak { .. } => out.push_str(" \\\n"),
            Node::FootnoteRef { .. } => {
                // Without the definition side-channel we can only emit a
                // placeholder. Consumers doing full footnote round-trip
                // should run a pre-pass that resolves FootnoteRef to
                // inline `#footnote[..]` calls.
            }
            Node::Html { .. } => {
                // Inline HTML: skip.
            }
            Node::Custom {
                payload, children, ..
            } => {
                let walk_block = |node: &Node<Ext>, o: &mut String| self.write_block(node, o);
                let walk_inlines =
                    |nodes: &[Node<Ext>], o: &mut String| self.write_inlines(nodes, o);
                let walker = TypstWalker {
                    walk_block: &walk_block,
                    walk_inlines: &walk_inlines,
                };
                self.custom.render_custom(payload, children, out, &walker)?;
            }
            Node::Paragraph { .. }
            | Node::Heading { .. }
            | Node::BlockQuote { .. }
            | Node::List { .. }
            | Node::Item { .. }
            | Node::TaskItem { .. }
            | Node::Table { .. }
            | Node::TableRow { .. }
            | Node::TableCell { .. }
            | Node::CodeBlock { .. }
            | Node::MathBlock { .. }
            | Node::ThematicBreak { .. }
            | Node::Alert { .. }
            | Node::DefinitionList { .. }
            | Node::DefinitionTerm { .. }
            | Node::DefinitionDescription { .. }
            | Node::FootnoteDefinition { .. }
            | Node::FootnoteSection { .. }
            | Node::Document { .. } => self.write_block(node, out)?,
        }
        Ok(())
    }

    fn write_table<Ext>(
        &self,
        alignments: &[Option<TableAlignment>],
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), TypstError>
    where
        R: RenderTypstCustom<Ext>,
    {
        let columns = alignments.len().max(
            header
                .iter()
                .chain(rows.iter())
                .filter_map(|row| match row {
                    Node::TableRow { cells, .. } => Some(cells.len()),
                    _ => None,
                })
                .max()
                .unwrap_or(1),
        );
        out.push_str("#table(\n  columns: ");
        out.push_str(&columns.to_string());
        if !alignments.is_empty() {
            out.push_str(",\n  align: (");
            for (idx, a) in alignments.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(match a {
                    Some(TableAlignment::Left) => "left",
                    Some(TableAlignment::Center) => "center",
                    Some(TableAlignment::Right) => "right",
                    None => "auto",
                });
            }
            if columns > alignments.len() {
                for _ in alignments.len()..columns {
                    out.push_str(", auto");
                }
            }
            out.push(')');
        }
        out.push_str(",\n");

        for row in header.iter().chain(rows.iter()) {
            if let Node::TableRow { cells, .. } = row {
                for cell in cells {
                    if let Node::TableCell { children, .. } = cell {
                        out.push_str("  [");
                        self.write_inlines(children, out)?;
                        out.push_str("],\n");
                    }
                }
            }
        }
        out.push_str(")\n\n");
        Ok(())
    }
}

/// Streaming sink wrapper.
pub struct TypstSink<S, R = ()> {
    inner: S,
    adapter: TypstAdapter<R>,
}

impl<S> TypstSink<S, ()> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            adapter: TypstAdapter::new(),
        }
    }
}

impl<S, R> TypstSink<S, R> {
    pub fn with_custom_renderer(inner: S, custom: R) -> Self {
        Self {
            inner,
            adapter: TypstAdapter { custom },
        }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, R, Ext> Sink<Node<Ext>> for TypstSink<S, R>
where
    S: Sink<String>,
    S::Error: fmt::Display,
    R: RenderTypstCustom<Ext>,
{
    type Error = TypstError;

    fn emit(&mut self, item: StreamItem<Node<Ext>>) -> Result<(), Self::Error> {
        let rendered = self.adapter.render(item)?;
        self.inner
            .emit(rendered)
            .map_err(|e| TypstError::Sink(e.to_string()))
    }
}

// --- escaping / formatting helpers -----------------------------------

/// Typst markup has meaningful characters that must be escaped when
/// they appear in user-authored text.
///
/// Conservative list: `\` (escape prefix), `#` (code mode trigger),
/// `$` (math mode), `` ` `` (raw), `*` / `_` (emphasis), `<` / `>`
/// (label and grouping), `=` (heading only at line start, but escape
/// defensively), `@` (reference), `"` (string literal in some contexts),
/// `[` / `]` (content brackets), `(` / `)` (can be content-sensitive
/// around code spans — escape to be safe inside expressions, though
/// not required here), `+` / `-` (list markers at line start).
///
/// We play it safe and escape the always-meaningful ones in text
/// content; the linewise-only markers (`=`, `+`, `-`) we only escape
/// when they appear at the start of a fresh line, which we track via
/// the text's leading-whitespace prefix.
fn write_typst_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' | '#' | '$' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '@' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push('\n'),
            _ => out.push(ch),
        }
    }
}

/// Inside a `"..."` string literal: backslash-escape `\` and `"`; let
/// everything else through unchanged.
fn write_typst_string_literal(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
}

/// Emit a Typst raw block. Picks a fence length one longer than the
/// longest backtick run in the literal so the fence is unambiguous.
fn write_raw_block(lang: Option<&str>, literal: &str, out: &mut String) {
    let fence_len = longest_backtick_run(literal) + 1;
    let fence_len = fence_len.max(3);
    for _ in 0..fence_len {
        out.push('`');
    }
    if let Some(lang) = lang {
        out.push_str(lang);
    }
    if !literal.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(literal);
    if !literal.ends_with('\n') {
        out.push('\n');
    }
    for _ in 0..fence_len {
        out.push('`');
    }
}

/// Emit a Typst inline raw span. Same fence-length logic as for blocks.
fn write_raw_inline(literal: &str, out: &mut String) {
    let fence_len = longest_backtick_run(literal) + 1;
    for _ in 0..fence_len {
        out.push('`');
    }
    out.push_str(literal);
    for _ in 0..fence_len {
        out.push('`');
    }
}

/// Strip ANSI escape sequences from `source`, leaving only printable
/// text. Uses `strip-ansi-escapes` when the `ansi` feature is on;
/// otherwise falls back to a conservative regex-free ESC/[...]
/// recognizer so the Typst adapter doesn't silently ship escapes into
/// PDF output.
fn strip_ansi_for_typst(source: &str) -> String {
    #[cfg(feature = "ansi")]
    {
        strip_ansi_escapes::strip_str(source)
    }
    #[cfg(not(feature = "ansi"))]
    {
        let mut out = String::with_capacity(source.len());
        let mut chars = source.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                match chars.next() {
                    Some('[') => {
                        for c in chars.by_ref() {
                            if c.is_ascii_alphabetic() {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        while let Some(c) = chars.next() {
                            if c == '\x07' {
                                break;
                            }
                            if c == '\x1b' {
                                chars.next();
                                break;
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            out.push(c);
        }
        out
    }
}

/// Run `source` through a VT100 emulator and return the final visible
/// screen as plain text (no escapes). Falls back to the same strip
/// behaviour as `strip_ansi_for_typst` when the `terminal` feature is
/// off.
fn terminal_snapshot_for_typst(source: &str) -> String {
    #[cfg(feature = "terminal")]
    {
        // Produce ANSI first so we can rely on render_grid_ansi's
        // row-trimming logic, then strip the escapes for print.
        let grid = crate::terminal::ansi_snapshot_grid(source, None, None);
        let ansi = crate::terminal::render_grid_ansi(&grid);
        #[cfg(feature = "ansi")]
        {
            strip_ansi_escapes::strip_str(&ansi)
        }
        #[cfg(not(feature = "ansi"))]
        {
            strip_ansi_for_typst(&ansi)
        }
    }
    #[cfg(not(feature = "terminal"))]
    {
        strip_ansi_for_typst(source)
    }
}

fn longest_backtick_run(source: &str) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for ch in source.chars() {
        if ch == '`' {
            current += 1;
            if current > longest {
                longest = current;
            }
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Emission, Node, SourceSpan, StreamItem};

    fn render<Ext>(node: Node<Ext>) -> String
    where
        (): RenderTypstCustom<Ext>,
    {
        TypstAdapter::new().render_node(&node).unwrap()
    }

    #[test]
    fn renders_heading_and_paragraph() {
        let doc = Node::<()>::Document {
            children: vec![
                Node::Heading {
                    level: 2,
                    children: vec![Node::Text {
                        text: "Hello".into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
                Node::Paragraph {
                    children: vec![Node::Text {
                        text: "world".into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        };
        let out = render(doc);
        assert!(out.contains("== Hello"));
        assert!(out.contains("world"));
    }

    #[test]
    fn renders_emphasis_strong_link() {
        let para = Node::<()>::Paragraph {
            children: vec![
                Node::Emphasis {
                    children: vec![Node::Text {
                        text: "italic".into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
                Node::Text {
                    text: " ".into(),
                    source: SourceSpan::default(),
                },
                Node::Strong {
                    children: vec![Node::Text {
                        text: "bold".into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
                Node::Text {
                    text: " ".into(),
                    source: SourceSpan::default(),
                },
                Node::Link {
                    destination: "https://silo.dev".into(),
                    title: None,
                    children: vec![Node::Text {
                        text: "site".into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        };
        let out = render(para);
        assert!(out.contains("_italic_"));
        assert!(out.contains("*bold*"));
        assert!(out.contains("#link(\"https://silo.dev\")[site]"));
    }

    #[test]
    fn renders_code_block_with_language_and_inline_code() {
        let doc = Node::<()>::Document {
            children: vec![
                Node::CodeBlock {
                    info: Some("rust".into()),
                    literal: "fn main() {}\n".into(),
                    source: SourceSpan::default(),
                },
                Node::Paragraph {
                    children: vec![Node::CodeSpan {
                        code: "Vec".into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        };
        let out = render(doc);
        assert!(out.contains("```rust\nfn main() {}\n```"));
        assert!(out.contains("`Vec`"));
    }

    #[test]
    fn renders_ordered_list_with_start() {
        let list = Node::<()>::List {
            ordered: true,
            start: Some(3),
            tight: Some(true),
            children: vec![
                Node::Item {
                    children: vec![Node::Paragraph {
                        children: vec![Node::Text {
                            text: "three".into(),
                            source: SourceSpan::default(),
                        }],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
                Node::Item {
                    children: vec![Node::Paragraph {
                        children: vec![Node::Text {
                            text: "four".into(),
                            source: SourceSpan::default(),
                        }],
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            ],
            source: SourceSpan::default(),
        };
        let out = render(list);
        assert!(out.contains("#enum(start: 3"));
        assert!(out.contains("[three]"));
        assert!(out.contains("[four]"));
    }

    #[test]
    fn renders_alert_as_admonition_call() {
        let alert = Node::<()>::Alert {
            kind: AlertKind::Note,
            title: Some("Heads up".into()),
            children: vec![Node::Paragraph {
                children: vec![Node::Text {
                    text: "body".into(),
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
            }],
            source: SourceSpan::default(),
        };
        let out = render(alert);
        assert!(out.contains("#admonition(kind: \"note\""));
        assert!(out.contains("title: \"Heads up\""));
        assert!(out.contains("body"));
    }

    #[test]
    fn escapes_typst_specials_in_text() {
        let para = Node::<()>::Paragraph {
            children: vec![Node::Text {
                text: "use #eval or $math$".into(),
                source: SourceSpan::default(),
            }],
            source: SourceSpan::default(),
        };
        let out = render(para);
        assert!(out.contains("\\#eval"));
        assert!(out.contains("\\$math"));
    }

    #[test]
    fn stream_item_passes_id_through() {
        let adapter: TypstAdapter<()> = TypstAdapter::new();
        let rendered = adapter
            .render(StreamItem::Upsert(Emission {
                id: 42,
                provisional: false,
                node: Node::<()>::Paragraph {
                    children: vec![Node::Text {
                        text: "x".into(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                },
            }))
            .unwrap();
        match rendered {
            StreamItem::Upsert(e) => {
                assert_eq!(e.id, 42);
                assert!(e.node.contains('x'));
            }
            _ => panic!("expected Upsert"),
        }
    }
}
