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
    /// Block ID currently being shown in raw-source form (cursor inside).
    cursor_block_id: Option<CorrelationId>,
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
            cursor_block_id: None,
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
    ///
    /// Internally uses block-window reparse: only blocks at or after the
    /// edit position are reparsed; unchanged prefix blocks keep their IDs
    /// and are not re-emitted.
    pub fn edit(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        let edit_start = range.start;
        self.source.replace_range(range, replacement);
        self.reparse_from(edit_start)
    }

    /// Move the editing cursor to `byte_offset` (or `None` to clear). Returns
    /// the events the renderer should consume to switch the previously-
    /// cursored block back to its formatted rendering and the newly-
    /// cursored block to a raw-source rendering (a `Node::CodeBlock` with
    /// `info = "markdown"`).
    ///
    /// This is the Bear-style "reveal source under cursor" pattern.
    /// Adapters that don't want this behaviour can ignore the cursor.
    pub fn set_cursor(&mut self, byte_offset: Option<usize>) -> Vec<StreamItem<Node<Ext>>> {
        use crate::node::{Emission, SourceSpan};

        let new_id = byte_offset.and_then(|c| self.block_at(c)).map(|b| b.id);
        if new_id == self.cursor_block_id {
            return Vec::new();
        }
        let mut events = Vec::new();

        // Re-emit the previous cursor block as formatted.
        if let Some(prev_id) = self.cursor_block_id {
            if let Some(block) = self.blocks.iter().find(|b| b.id == prev_id) {
                events.push(StreamItem::Upsert(Emission {
                    id: prev_id,
                    provisional: block.provisional,
                    node: block.node.clone(),
                }));
            }
        }

        // Emit the new cursor block as raw markdown source.
        if let Some(id) = new_id {
            if let Some(block) = self.blocks.iter().find(|b| b.id == id) {
                let span = block.node.source();
                let raw = self
                    .source
                    .get(span.start..span.end)
                    .unwrap_or("")
                    .trim_end_matches('\n')
                    .to_string();
                events.push(StreamItem::Upsert(Emission {
                    id,
                    provisional: block.provisional,
                    node: Node::CodeBlock {
                        info: Some("markdown".to_string()),
                        literal: raw,
                        source: SourceSpan::new(span.start, span.end),
                    },
                }));
            }
        }

        self.cursor_block_id = new_id;
        events
    }

    fn reparse(&mut self) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        self.reparse_from(0)
    }

    /// Reparse from the first block whose source span overlaps `edit_start`
    /// onward. Blocks before that point are left untouched.
    fn reparse_from(&mut self, edit_start: usize) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        let split_idx = self
            .blocks
            .iter()
            .position(|b| b.node.source().end > edit_start)
            .unwrap_or(self.blocks.len());

        let prefix_byte_end = if split_idx == 0 {
            0
        } else {
            self.blocks[split_idx - 1].node.source().end
        };

        let suffix_source = &self.source[prefix_byte_end..];
        let suffix_blocks = self.parser.parse_full_at(suffix_source, prefix_byte_end)?;

        let old_suffix: Vec<LiveBlock<Ext>> = self.blocks.drain(split_idx..).collect();
        let (new_suffix_live, events) =
            reconcile_blocks(old_suffix, suffix_blocks, &mut self.next_id);
        self.blocks.extend(new_suffix_live);
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
    fn inline_code_span_maps_back_to_source() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("hello `code` end").unwrap();
        let para = &doc.blocks()[0].node;
        let Node::Paragraph { children, .. } = para else {
            panic!("expected paragraph");
        };
        let code = children
            .iter()
            .find(|c| matches!(c, Node::CodeSpan { .. }))
            .expect("expected code span");
        let span = code.source();
        let slice = &doc.source()[span.start..span.end];
        assert_eq!(slice, "`code`");
    }

    #[test]
    fn block_at_returns_none_for_blank_line_between_blocks() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\n\nsecond").unwrap();
        // Offset 6 is the "\n" between blocks
        assert!(doc.block_at(6).is_none());
    }

    #[test]
    fn cursor_swaps_block_to_raw_source_and_back() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("hello *world*\n\nsecond").unwrap();
        let first_id = doc.blocks()[0].id;

        // Move cursor inside the first block.
        let events = doc.set_cursor(Some(3));
        // Should emit one Upsert: the first block as raw CodeBlock.
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamItem::Upsert(em) => {
                assert_eq!(em.id, first_id);
                assert!(matches!(
                    em.node,
                    Node::CodeBlock {
                        info: Some(ref s),
                        ..
                    } if s == "markdown"
                ));
            }
            _ => panic!("expected Upsert"),
        }

        // Move cursor away — should emit two Upserts: first block back to
        // formatted (Paragraph), no second swap.
        let events = doc.set_cursor(Some(15));
        assert_eq!(events.len(), 2);
        match &events[0] {
            StreamItem::Upsert(em) => {
                assert_eq!(em.id, first_id);
                assert!(matches!(em.node, Node::Paragraph { .. }));
            }
            _ => panic!("expected Upsert for previous cursor block"),
        }
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
