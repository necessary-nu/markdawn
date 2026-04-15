//! Reactive markdown editor with adapter-agnostic edit semantics.
//!
//! `EditableDocument` wraps a source buffer and a `Parser`, reparsing on every
//! edit. It emits the same `StreamItem::Upsert/Delete` events as the streaming
//! parser, so any sink that consumes the streaming protocol (UIKit, JS, HTML)
//! works unchanged.
//!
//! ```ignore
//! let mut doc = EditableDocument::new(Parser::builder().with_gfm().build());
//! let events = doc.set_source("# Hello\n\nWorld");
//! // feed events into BlockSink, HtmlSink, etc.
//!
//! let events = doc.edit(7..7, " there");  // insert "there" after "Hello"
//! ```
//!
//! ## Performance
//!
//! v1 reparses the whole document on every edit. For chat-sized content
//! (under ~10KB), this is microseconds and adequate. Block-window reparse
//! that touches only the affected block is a future optimization gated on
//! source span population.
//!
//! ## Spans
//!
//! Top-level blocks have populated `source` spans (byte ranges in the
//! source). Inline nodes inside blocks still have `0..0` spans — that's a
//! follow-up. `block_at(byte_offset)` works against top-level blocks today.

use std::ops::Range;

use crate::error::Error;
use crate::node::{CorrelationId, Node, StreamItem};
use crate::parser::{LiveBlock, Parser, reconcile_blocks};

/// Live document backed by a markdown source buffer. Edits produce
/// `StreamItem` events suitable for the same sinks the streaming parser uses.
pub struct EditableDocument<Ext = ()> {
    source: String,
    blocks: Vec<LiveBlock<Ext>>,
    parser: Parser<Ext>,
    next_id: CorrelationId,
}

impl<Ext> EditableDocument<Ext>
where
    Ext: Clone + PartialEq,
{
    /// Wrap the given parser. The parser's streaming state is reset on every
    /// reparse — only its configuration (extensions, GFM/math/alerts/etc.
    /// flags) is retained.
    pub fn new(parser: Parser<Ext>) -> Self {
        Self {
            source: String::new(),
            blocks: Vec::new(),
            parser,
            next_id: 1,
        }
    }

    /// Current source buffer.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Current top-level blocks with their correlation IDs.
    pub fn blocks(&self) -> &[LiveBlock<Ext>] {
        &self.blocks
    }

    /// Return the top-level block whose source span contains `byte_offset`.
    ///
    /// Returns `None` if the offset falls in inter-block whitespace or past
    /// the end of the source.
    pub fn block_at(&self, byte_offset: usize) -> Option<&LiveBlock<Ext>> {
        self.blocks
            .iter()
            .find(|b| b.node.source().contains(byte_offset))
    }

    /// Replace the entire source buffer and reparse. Returns the events the
    /// renderer should consume to reach the new state.
    pub fn set_source(&mut self, source: &str) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        self.source.clear();
        self.source.push_str(source);
        self.reparse()
    }

    /// Apply a text edit: replace `range` in the source with `replacement`.
    /// Returns the events the renderer should consume.
    ///
    /// `range` is in byte offsets into the current source. Panics if the
    /// range is invalid (out of bounds or not on a UTF-8 boundary).
    pub fn edit(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        self.source.replace_range(range, replacement);
        self.reparse()
    }

    fn reparse(&mut self) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        let blocks = self.parser.parse_full(&self.source)?;
        let previous = std::mem::take(&mut self.blocks);
        let (new_blocks, events) = reconcile_blocks(previous, blocks, &mut self.next_id);
        self.blocks = new_blocks;
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Emission, Node, StreamItem};
    use crate::parser::Parser;

    fn extract_paragraph_text(node: &Node<()>) -> Option<String> {
        match node {
            Node::Paragraph { children, .. } => {
                let mut out = String::new();
                for child in children {
                    if let Node::Text { text, .. } = child {
                        out.push_str(text);
                    }
                }
                Some(out)
            }
            _ => None,
        }
    }

    #[test]
    fn empty_document_emits_nothing() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let events = doc.set_source("").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn initial_source_emits_one_upsert_per_block() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let events = doc.set_source("# Title\n\nBody").unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], StreamItem::Upsert(_)));
        assert!(matches!(events[1], StreamItem::Upsert(_)));
    }

    #[test]
    fn editing_a_paragraph_emits_only_an_update_for_that_paragraph() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\n\nsecond\n\nthird").unwrap();

        // Edit "second" → "SECOND"
        let events = doc.edit(7..13, "SECOND").unwrap();
        let upserts: Vec<&Emission<Node<()>>> = events
            .iter()
            .filter_map(|e| match e {
                StreamItem::Upsert(em) => Some(em),
                _ => None,
            })
            .collect();
        assert_eq!(
            upserts.len(),
            1,
            "expected only the changed paragraph to upsert"
        );
        assert_eq!(
            extract_paragraph_text(&upserts[0].node).as_deref(),
            Some("SECOND")
        );
    }

    #[test]
    fn appending_text_emits_only_one_upsert_for_the_last_block() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\n\nsecond").unwrap();
        let events = doc.edit(13..13, "!").unwrap();
        let upserts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamItem::Upsert(_)))
            .collect();
        assert_eq!(upserts.len(), 1);
    }

    #[test]
    fn deleting_a_block_emits_a_delete_event() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("alpha\n\nbeta\n\ngamma").unwrap();
        // Remove the middle paragraph + its trailing blank line
        let events = doc.edit(7..13, "").unwrap();
        let deletes: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamItem::Delete { .. }))
            .collect();
        assert_eq!(deletes.len(), 1);
    }

    #[test]
    fn block_id_is_stable_across_unrelated_edits() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("alpha\n\nbeta").unwrap();
        let alpha_id = doc.blocks()[0].id;
        // Edit only the second paragraph
        let _ = doc.edit(7..11, "BETA").unwrap();
        assert_eq!(
            doc.blocks()[0].id,
            alpha_id,
            "first block ID should be preserved"
        );
    }

    #[test]
    fn block_at_finds_block_under_cursor() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\n\nsecond\n\nthird").unwrap();
        // Source: "first\n\nsecond\n\nthird"
        //          0     5 6 7  ↑
        // Offset 8 is inside "second" → second block
        let block = doc.block_at(8).expect("expected a block at offset 8");
        let text = extract_paragraph_text(&block.node).unwrap();
        assert_eq!(text, "second");
    }

    #[test]
    fn block_at_returns_none_for_blank_line_between_blocks() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\n\nsecond").unwrap();
        // Offset 6 is the "\n" between blocks
        assert!(doc.block_at(6).is_none());
    }

    #[test]
    fn typing_a_blockquote_marker_converts_paragraph_to_blockquote() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("hello").unwrap();
        // Insert "> " at the start
        let events = doc.edit(0..0, "> ").unwrap();
        // Should produce an Upsert with a BlockQuote node
        let upsert = events.iter().find_map(|e| match e {
            StreamItem::Upsert(em) => Some(em),
            _ => None,
        });
        assert!(matches!(upsert.unwrap().node, Node::BlockQuote { .. }));
    }
}
