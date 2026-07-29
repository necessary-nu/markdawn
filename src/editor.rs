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
//! Top-level blocks carry `source` spans covering their exact byte range in
//! the buffer, and the inline nodes inside them carry spans that slice back
//! to their own source text, so an emphasis, strong or code span can be cut
//! straight out of `source()`. `block_at(byte_offset)` resolves an offset
//! against the top-level blocks.
//!
//! The buffer is normalised on ingest — CRLF and lone CR to LF, NUL to
//! U+FFFD — because the parser computes spans against normalised text. The
//! document therefore stores what it parsed rather than the caller's exact
//! bytes, and every offset crossing the API is an offset into that
//! normalised text.

use std::ops::Range;

use crate::error::Error;
use crate::node::{CorrelationId, Emission, Node, StreamItem};
use crate::parser::{LiveBlock, Parser, normalize_chunk, reconcile_blocks};

/// Live document backed by a markdown source buffer. Edits produce
/// `StreamItem` events suitable for the same sinks the streaming parser uses.
// [spec:mw:def:editing.document.editable-document]
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

    /// Current source buffer, normalised: line endings are LF and NUL is
    /// U+FFFD. Every span the document hands out indexes this string.
    // [spec:mw:req:editing.document.byte-offsets+1]
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
    // [spec:mw:sem:editing.block-at.containment]
    pub fn block_at(&self, byte_offset: usize) -> Option<&LiveBlock<Ext>> {
        self.blocks
            .iter()
            .find(|b| b.node.source().contains(byte_offset))
    }

    /// Replace the entire source buffer and reparse. Returns the events the
    /// renderer should consume to reach the new state.
    ///
    /// The text is normalised before it is stored, so `source()` will not
    /// echo `source` byte for byte when it holds CRLF, a lone CR, or a NUL.
    // [spec:mw:req:editing.reparse.window+1]
    // [spec:mw:req:editing.document.byte-offsets+1]
    pub fn set_source(&mut self, source: &str) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        self.source = normalize_chunk(source);
        self.reparse()
    }

    /// Apply a text edit: replace `range` in the source with `replacement`.
    /// Returns the events the renderer should consume.
    ///
    /// `range` is in byte offsets into the current — normalised — source.
    /// Panics if the range is invalid (out of bounds or not on a UTF-8
    /// boundary). The spliced buffer is renormalised, so a replacement
    /// carrying CRLF or NUL is stored in its normalised form.
    ///
    /// Internally uses block-window reparse: only blocks at or after the
    /// edit position are reparsed; unchanged prefix blocks keep their IDs
    /// and are not re-emitted.
    // [spec:mw:req:editing.document.byte-offsets+1]
    pub fn edit(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        let edit_start = range.start;
        self.source.replace_range(range, replacement);
        self.source = normalize_chunk(&self.source);
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
    // [spec:mw:req:editing.cursor.reveal-source]
    // [spec:mw:sem:editing.cursor.reparse-edit-interaction+1]
    pub fn set_cursor(&mut self, byte_offset: Option<usize>) -> Vec<StreamItem<Node<Ext>>> {
        use crate::node::SourceSpan;

        let new_id = byte_offset.and_then(|c| self.block_at(c)).map(|b| b.id);
        if new_id == self.cursor_block_id {
            return Vec::new();
        }
        let mut events = Vec::new();

        // Re-emit the previous cursor block as formatted.
        if let Some(prev_id) = self.cursor_block_id
            && let Some(block) = self.blocks.iter().find(|b| b.id == prev_id)
        {
            events.push(StreamItem::Upsert(Emission {
                id: prev_id,
                provisional: block.provisional,
                node: block.node.clone(),
            }));
        }

        // Emit the new cursor block as raw markdown source.
        if let Some(id) = new_id
            && let Some(block) = self.blocks.iter().find(|b| b.id == id)
        {
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

        self.cursor_block_id = new_id;
        events
    }

    fn reparse(&mut self) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        self.reparse_from(0)
    }

    /// Reparse from the first block whose source span reaches `edit_start`
    /// onward. Blocks before that point are left untouched. A block whose
    /// span ends exactly at `edit_start` is inside the window: text spliced
    /// in at that offset extends it rather than following it.
    // [spec:mw:req:editing.reparse.window+1]
    // [spec:mw:req:editing.reparse.correlation-stability]
    fn reparse_from(&mut self, edit_start: usize) -> Result<Vec<StreamItem<Node<Ext>>>, Error> {
        let split_idx = self
            .blocks
            .iter()
            .position(|b| b.node.source().end >= edit_start)
            .unwrap_or(self.blocks.len());

        let prefix_byte_end = if split_idx == 0 {
            0
        } else {
            self.blocks[split_idx - 1].node.source().end
        };

        let suffix_source = &self.source[prefix_byte_end..];
        let suffix_blocks = self.parser.parse_full_at(suffix_source, prefix_byte_end)?;

        let old_suffix: Vec<LiveBlock<Ext>> = self.blocks.drain(split_idx..).collect();
        let (new_suffix_live, mut events) =
            reconcile_blocks(old_suffix, suffix_blocks, &mut self.next_id);
        self.blocks.extend(new_suffix_live);
        self.end_cursor_reveal(&mut events);
        Ok(events)
    }

    /// A reparse ends the cursor reveal: correlation ids are positional, so
    /// the id the cursor was resolved to may name a different block after
    /// the reparse, and holding it would re-raw the wrong one. Drop it, and
    /// make sure the block that was showing raw source is put back to its
    /// formatted node — the reconcile only emits blocks that changed, so a
    /// block that reparsed identically would otherwise stay raw forever.
    // [spec:mw:sem:editing.cursor.reparse-edit-interaction+1]
    fn end_cursor_reveal(&mut self, events: &mut Vec<StreamItem<Node<Ext>>>) {
        let Some(previous) = self.cursor_block_id.take() else {
            return;
        };
        let already_emitted = events
            .iter()
            .any(|event| matches!(event, StreamItem::Upsert(em) if em.id == previous));
        if already_emitted {
            return;
        }
        let Some(block) = self.blocks.iter().find(|b| b.id == previous) else {
            return;
        };
        let restore = StreamItem::Upsert(Emission {
            id: previous,
            provisional: block.provisional,
            node: block.node.clone(),
        });
        // Upserts precede deletes within a batch.
        // [spec:mw:req:core.stream.batch-ordering]
        let insert_at = events
            .iter()
            .position(|event| matches!(event, StreamItem::Delete { .. }))
            .unwrap_or(events.len());
        events.insert(insert_at, restore);
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

    // [spec:mw:def:editing.document.editable-document/test]
    // [spec:mw:def:core.stream.protocol/test]
    #[test]
    fn initial_source_emits_one_upsert_per_block() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let events = doc.set_source("# Title\n\nBody").unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], StreamItem::Upsert(_)));
        assert!(matches!(events[1], StreamItem::Upsert(_)));
    }

    // [spec:mw:req:editing.document.byte-offsets+1/test]
    // [spec:mw:req:core.stream.change-only-upsert/test]
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

    // [spec:mw:req:editing.reparse.window+1/test]
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

    // [spec:mw:req:editing.reparse.correlation-stability/test]
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

    // [spec:mw:req:editing.reparse.correlation-stability/test]
    // [spec:mw:req:core.stream.slot-identity/test]
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

    // [spec:mw:sem:editing.block-at.containment/test]
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

    // [spec:mw:def:core.span.half-open-bytes/test]
    #[test]
    fn inline_emphasis_span_maps_back_to_source() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("hello *world* end").unwrap();
        let para = &doc.blocks()[0].node;
        let Node::Paragraph { children, .. } = para else {
            panic!("expected paragraph");
        };
        let emphasis = children
            .iter()
            .find(|c| matches!(c, Node::Emphasis { .. }))
            .expect("expected emphasis child");
        let span = emphasis.source();
        assert_eq!(&doc.source()[span.start..span.end], "*world*");
    }

    // [spec:mw:req:core.span.exact-extent/test]
    #[test]
    fn inline_strong_span_maps_back_to_source() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("a **bold** b").unwrap();
        let Node::Paragraph { children, .. } = &doc.blocks()[0].node else {
            panic!("expected paragraph");
        };
        let strong = children
            .iter()
            .find(|c| matches!(c, Node::Strong { .. }))
            .expect("expected strong child");
        let span = strong.source();
        assert_eq!(&doc.source()[span.start..span.end], "**bold**");
    }

    // [spec:mw:req:core.span.sliceable/test]
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

    // [spec:mw:sem:editing.block-at.containment/test]
    // [spec:mw:sem:core.span.contains/test]
    #[test]
    fn block_at_returns_none_for_blank_line_between_blocks() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\n\nsecond").unwrap();
        // Offset 6 is the "\n" between blocks
        assert!(doc.block_at(6).is_none());
    }

    // [spec:mw:req:editing.cursor.reveal-source/test]
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

    fn slice<'a>(doc: &'a EditableDocument<()>, node: &Node<()>) -> &'a str {
        let span = node.source();
        &doc.source()[span.start..span.end]
    }

    fn emphasis_of(node: &Node<()>) -> &Node<()> {
        let Node::Paragraph { children, .. } = node else {
            panic!("expected paragraph");
        };
        children
            .iter()
            .find(|c| matches!(c, Node::Emphasis { .. }))
            .expect("expected emphasis child")
    }

    // [spec:mw:req:editing.document.byte-offsets+1/test]
    // [spec:mw:req:core.span.sliceable/test]
    #[test]
    fn crlf_document_spans_index_the_stored_buffer() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc
            .set_source("first *one*\r\n\r\nsecond *two*\r\n\r\nthird *three*\r\n")
            .unwrap();
        assert_eq!(
            doc.source(),
            "first *one*\n\nsecond *two*\n\nthird *three*\n"
        );

        // Each line ending costs a byte, so the last block is where any
        // mismatch between the buffer and the spans would be widest.
        let blocks: Vec<&Node<()>> = doc.blocks().iter().map(|b| &b.node).collect();
        assert_eq!(slice(&doc, blocks[0]), "first *one*\n");
        assert_eq!(slice(&doc, blocks[1]), "second *two*\n");
        assert_eq!(slice(&doc, blocks[2]), "third *three*\n");
        assert_eq!(slice(&doc, emphasis_of(blocks[2])), "*three*");
    }

    // [spec:mw:req:editing.document.byte-offsets+1/test]
    // [spec:mw:req:core.span.sliceable/test]
    #[test]
    fn nul_document_spans_index_the_stored_buffer() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("a\0b\n\nsecond *two*\n").unwrap();
        assert_eq!(doc.source(), "a\u{FFFD}b\n\nsecond *two*\n");

        let blocks: Vec<&Node<()>> = doc.blocks().iter().map(|b| &b.node).collect();
        assert_eq!(slice(&doc, blocks[0]), "a\u{FFFD}b\n");
        assert_eq!(slice(&doc, blocks[1]), "second *two*\n");
        assert_eq!(slice(&doc, emphasis_of(blocks[1])), "*two*");
    }

    // [spec:mw:req:editing.document.byte-offsets+1/test]
    #[test]
    fn edit_offsets_are_read_against_the_normalised_buffer() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\r\n\r\nsecond").unwrap();
        // 7 is "second"'s first byte in the normalised buffer; in the text
        // the caller supplied it is the "e".
        let _ = doc.edit(7..13, "SECOND").unwrap();
        assert_eq!(doc.source(), "first\n\nSECOND");
    }

    // [spec:mw:req:editing.document.byte-offsets+1/test]
    #[test]
    fn a_spliced_crlf_replacement_is_normalised_too() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first").unwrap();
        let _ = doc.edit(5..5, "\r\n\r\nsecond *two*\n").unwrap();
        assert_eq!(doc.source(), "first\n\nsecond *two*\n");
        let blocks: Vec<&Node<()>> = doc.blocks().iter().map(|b| &b.node).collect();
        assert_eq!(slice(&doc, emphasis_of(blocks[1])), "*two*");
    }

    // [spec:mw:req:editing.reparse.window+1/test]
    #[test]
    fn appending_at_the_buffer_end_extends_the_final_block() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("first\n\nsecond").unwrap();
        let _ = doc.edit(13..13, "!").unwrap();
        assert_eq!(doc.blocks().len(), 2, "append must not split off a block");
        assert_eq!(
            extract_paragraph_text(&doc.blocks()[1].node).as_deref(),
            Some("second!")
        );
    }

    // [spec:mw:sem:editing.cursor.reparse-edit-interaction+1/test]
    #[test]
    fn an_edit_restores_the_cursored_block_and_releases_the_cursor() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("hello *world*\n\nsecond\n").unwrap();
        let first_id = doc.blocks()[0].id;
        assert_eq!(doc.set_cursor(Some(3)).len(), 1);

        // Edit the *second* block: the first is outside the re-parse window,
        // so only the cursor teardown can put it back to formatted.
        let events = doc.edit(15..21, "SECOND").unwrap();
        let restored = events
            .iter()
            .find_map(|e| match e {
                StreamItem::Upsert(em) if em.id == first_id => Some(em),
                _ => None,
            })
            .expect("expected the cursored block to be restored");
        assert!(matches!(restored.node, Node::Paragraph { .. }));

        // The cursor is released, so re-issuing it at the same offset is not
        // short-circuited and the raw rendering comes back.
        let events = doc.set_cursor(Some(3));
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamItem::Upsert(em) => {
                assert_eq!(em.id, first_id);
                assert!(matches!(
                    em.node,
                    Node::CodeBlock { info: Some(ref s), .. } if s == "markdown"
                ));
            }
            _ => panic!("expected Upsert"),
        }
    }

    // [spec:mw:sem:editing.cursor.reparse-edit-interaction+1/test]
    // [spec:mw:req:core.stream.batch-ordering/test]
    #[test]
    fn cursor_teardown_does_not_duplicate_or_trail_a_delete() {
        let mut doc = EditableDocument::new(Parser::<()>::new());
        let _ = doc.set_source("alpha\n\nbeta\n\ngamma\n").unwrap();
        let second_id = doc.blocks()[1].id;
        assert_eq!(doc.set_cursor(Some(8)).len(), 1);

        // Delete the cursored paragraph's text and the block after it.
        let events = doc.edit(7..19, "BETA\n").unwrap();
        let upserts = events
            .iter()
            .filter(|e| matches!(e, StreamItem::Upsert(em) if em.id == second_id))
            .count();
        assert_eq!(upserts, 1, "the re-parse already emitted for this slot");
        let first_delete = events
            .iter()
            .position(|e| matches!(e, StreamItem::Delete { .. }));
        if let Some(first_delete) = first_delete {
            assert!(
                events[first_delete..]
                    .iter()
                    .all(|e| matches!(e, StreamItem::Delete { .. })),
                "upserts must precede deletes"
            );
        }
    }

    // [spec:mw:sem:core.span.unpopulated-is-empty+1/test]
    #[test]
    fn table_row_and_cell_spans_slice_their_source() {
        let mut doc = EditableDocument::new(Parser::<()>::builder().with_gfm().build());
        let _ = doc
            .set_source("intro\n\n| a | *bb* |\n| --- | --- |\n| ccc | d |\n")
            .unwrap();

        let Node::Table { header, rows, .. } = &doc.blocks()[1].node else {
            panic!("expected a table");
        };
        let Node::TableRow { cells, source } = &header[0] else {
            panic!("expected a header row");
        };
        assert_eq!(&doc.source()[source.start..source.end], "| a | *bb* |");
        assert_eq!(slice(&doc, &cells[0]), "a");
        assert_eq!(slice(&doc, &cells[1]), "*bb*");
        let Node::TableCell { children, .. } = &cells[1] else {
            panic!("expected a cell");
        };
        assert_eq!(slice(&doc, &children[0]), "*bb*");

        let Node::TableRow { cells, source } = &rows[0] else {
            panic!("expected a body row");
        };
        assert_eq!(&doc.source()[source.start..source.end], "| ccc | d |");
        assert_eq!(slice(&doc, &cells[0]), "ccc");
        assert_eq!(slice(&doc, &cells[1]), "d");
    }

    // [spec:mw:sem:core.span.unpopulated-is-empty+1/test]
    #[test]
    fn short_table_row_pads_with_empty_cells_at_the_row_end() {
        let mut doc = EditableDocument::new(Parser::<()>::builder().with_gfm().build());
        let _ = doc
            .set_source("| a | b |\n| --- | --- |\n| only |\n")
            .unwrap();

        let Node::Table { rows, .. } = &doc.blocks()[0].node else {
            panic!("expected a table");
        };
        let Node::TableRow { cells, source } = &rows[0] else {
            panic!("expected a body row");
        };
        assert_eq!(slice(&doc, &cells[0]), "only");
        let padded = cells[1].source();
        assert!(padded.is_empty());
        assert_eq!(padded.start, source.end, "padded cell sits at the row end");
    }

    // [spec:mw:sem:core.span.unpopulated-is-empty+1/test]
    #[test]
    fn footnote_definition_span_slices_its_source() {
        let mut doc = EditableDocument::new(Parser::<()>::builder().with_gfm().build());
        let _ = doc
            .set_source("text[^a]\n\n[^a]: the note\n\nafter\n")
            .unwrap();

        let section = doc
            .blocks()
            .iter()
            .find(|b| matches!(b.node, Node::FootnoteSection { .. }))
            .expect("expected a footnote section");
        let Node::FootnoteSection { children, source } = &section.node else {
            unreachable!();
        };
        // The section is assembled, not parsed: it has no extent, and sits
        // where it is appended rather than at some other block's start.
        assert!(source.is_empty());
        assert_eq!(source.start, doc.source().len());

        let Node::FootnoteDefinition { source, .. } = &children[0] else {
            panic!("expected a definition");
        };
        assert_eq!(
            &doc.source()[source.start..source.end],
            "[^a]: the note\n",
            "the definition spans where it was written, not where it was lifted to"
        );
    }
}
