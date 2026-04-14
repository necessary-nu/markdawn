pub mod attachment;
mod attrs;
mod content_storage;
mod element_map;
mod render;
mod style;
pub mod svg;
mod views;

pub use self::content_storage::MarkdownContentStorage;
pub use self::render::{RenderCtx, RenderTextKitCustom, TextKitAdapter, TextKitError};
pub use self::style::TextKitStyle;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::NSRange;
use objc2_ui_kit::NSTextStorageObserving;

use crate::node::{CorrelationId, Emission, Node, StreamItem};
use crate::sink::Sink;

use self::element_map::{ElementMap, ElementRecord, shift_ranges_after};

pub struct TextKitSink<R = ()> {
    adapter: TextKitAdapter<R>,
    storage: Retained<MarkdownContentStorage>,
    element_map: ElementMap,
}

impl TextKitSink<()> {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self {
            adapter: TextKitAdapter::new(),
            storage: MarkdownContentStorage::new(mtm),
            element_map: ElementMap::new(),
        }
    }

    pub fn with_style(mtm: MainThreadMarker, style: TextKitStyle) -> Self {
        Self {
            adapter: TextKitAdapter::with_style(std::sync::Arc::new(style)),
            storage: MarkdownContentStorage::new(mtm),
            element_map: ElementMap::new(),
        }
    }
}

impl<R> TextKitSink<R> {
    pub fn with_custom_renderer(mtm: MainThreadMarker, style: TextKitStyle, custom: R) -> Self {
        Self {
            adapter: TextKitAdapter::with_custom_renderer(std::sync::Arc::new(style), custom),
            storage: MarkdownContentStorage::new(mtm),
            element_map: ElementMap::new(),
        }
    }

    pub fn content_storage(&self) -> &MarkdownContentStorage {
        &self.storage
    }

    fn apply_upsert<Ext, Meta>(
        &mut self,
        emission: Emission<Node<Ext, Meta>>,
    ) -> Result<(), TextKitError>
    where
        R: RenderTextKitCustom<Ext, Meta>,
    {
        let ctx = RenderCtx {
            provisional: emission.provisional,
            ..Default::default()
        };
        let new_str = self.adapter.render(&emission.node, &ctx)?;
        let new_len = new_str.length();

        let text_storage = self
            .storage
            .textStorage()
            .expect("NSTextContentStorage must have a textStorage");

        if let Some(record) = self.element_map.get(&emission.id) {
            let old_range = record.range;
            let old_len = old_range.length;
            let storage_len = text_storage.length();

            // Bounds check: if the stored range is out of bounds, the element
            // map is stale. Remove the stale entry and treat as a new insert.
            if old_range.location + old_len > storage_len {
                self.element_map.swap_remove(&emission.id);
                let insert_loc = text_storage.length();
                let block = block2::StackBlock::new(|| {
                    text_storage.appendAttributedString(&new_str);
                });
                self.storage.performEditingTransactionUsingBlock(&block);
                self.element_map.insert(
                    emission.id,
                    ElementRecord {
                        range: NSRange::new(insert_loc, new_len),
                    },
                );
                return Ok(());
            }

            // Check if this is an append-only update (common in streaming):
            // the new content starts with the old content, so we only need
            // to insert the tail rather than replacing the whole block.
            let old_text = text_storage
                .attributedSubstringFromRange(old_range)
                .string()
                .to_string();
            let new_text = new_str.string().to_string();

            if new_len > old_len && new_text.starts_with(&old_text) {
                // Append-only: insert the new tail at the end of the existing range
                let tail_start = old_len;
                let tail = new_str
                    .attributedSubstringFromRange(NSRange::new(tail_start, new_len - old_len));
                let insert_point = old_range.location + old_len;
                let insert_range = NSRange::new(insert_point, 0);

                let block = block2::StackBlock::new(|| {
                    text_storage.replaceCharactersInRange_withAttributedString(insert_range, &tail);
                });
                self.storage.performEditingTransactionUsingBlock(&block);
            } else {
                // Content diverged — full replace
                let block = block2::StackBlock::new(|| {
                    text_storage.replaceCharactersInRange_withAttributedString(old_range, &new_str);
                });
                self.storage.performEditingTransactionUsingBlock(&block);
            }

            let delta = new_len as isize - old_len as isize;
            let after = old_range.location + old_len;
            shift_ranges_after(&mut self.element_map, after, delta);
            self.element_map.get_mut(&emission.id).unwrap().range.length = new_len;
        } else {
            let insert_loc = text_storage.length();

            let block = block2::StackBlock::new(|| {
                text_storage.appendAttributedString(&new_str);
            });
            self.storage.performEditingTransactionUsingBlock(&block);

            self.element_map.insert(
                emission.id,
                ElementRecord {
                    range: NSRange::new(insert_loc, new_len),
                },
            );
        }

        Ok(())
    }

    fn apply_delete(&mut self, id: CorrelationId) {
        if let Some(record) = self.element_map.swap_remove(&id) {
            let text_storage = self
                .storage
                .textStorage()
                .expect("NSTextContentStorage must have a textStorage");

            let range = record.range;
            let block = block2::StackBlock::new(|| {
                text_storage.deleteCharactersInRange(range);
            });
            self.storage.performEditingTransactionUsingBlock(&block);

            let after = record.range.location + record.range.length;
            shift_ranges_after(
                &mut self.element_map,
                after,
                -(record.range.length as isize),
            );
        }
    }
}

impl<R, Ext, Meta> Sink<Node<Ext, Meta>> for TextKitSink<R>
where
    R: RenderTextKitCustom<Ext, Meta>,
{
    type Error = TextKitError;

    fn emit(&mut self, item: StreamItem<Node<Ext, Meta>>) -> Result<(), Self::Error> {
        match item {
            StreamItem::Upsert(emission) => self.apply_upsert(emission),
            StreamItem::Delete { id } => {
                self.apply_delete(id);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::NSTextStorageObserving;

    use crate::node::{Emission, Node, StreamItem};
    use crate::sink::Sink;

    use super::TextKitSink;

    /// Get a `MainThreadMarker` for tests.
    ///
    /// # Safety
    ///
    /// These tests only manipulate `NSMutableAttributedString` / `NSTextStorage`
    /// in memory — no UIKit view hierarchy is involved. The `MainThreadOnly`
    /// constraint on `NSTextContentStorage` exists for UIKit coordination;
    /// the underlying storage operations are safe from any thread in a
    /// single-threaded test.
    fn test_mtm() -> MainThreadMarker {
        unsafe { MainThreadMarker::new_unchecked() }
    }

    fn text_of(sink: &TextKitSink) -> String {
        let ts = sink.content_storage().textStorage().unwrap();
        ts.string().to_string()
    }

    fn paragraph(id: u64, text: &str) -> StreamItem<Node<(), ()>> {
        StreamItem::Upsert(Emission {
            id,
            provisional: false,
            node: Node::Paragraph {
                children: vec![Node::Text {
                    text: text.into(),
                    meta: (),
                }],
                meta: (),
            },
        })
    }

    fn provisional_paragraph(id: u64, text: &str) -> StreamItem<Node<(), ()>> {
        StreamItem::Upsert(Emission {
            id,
            provisional: true,
            node: Node::Paragraph {
                children: vec![Node::Text {
                    text: text.into(),
                    meta: (),
                }],
                meta: (),
            },
        })
    }

    #[test]
    fn emits_paragraph_into_text_storage() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(paragraph(1, "hello world")).unwrap();
        assert_eq!(text_of(&sink), "hello world\n");
    }

    #[test]
    fn appends_multiple_blocks() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(paragraph(1, "first")).unwrap();
        sink.emit(paragraph(2, "second")).unwrap();
        assert_eq!(text_of(&sink), "first\nsecond\n");
    }

    #[test]
    fn upsert_replaces_existing_block() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(provisional_paragraph(1, "draft")).unwrap();
        assert_eq!(text_of(&sink), "draft\n");

        sink.emit(paragraph(1, "final version")).unwrap();
        assert_eq!(text_of(&sink), "final version\n");
    }

    #[test]
    fn delete_removes_block() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(paragraph(1, "keep")).unwrap();
        sink.emit(paragraph(2, "remove me")).unwrap();
        assert_eq!(text_of(&sink), "keep\nremove me\n");

        sink.emit(StreamItem::<Node<(), ()>>::Delete { id: 2 })
            .unwrap();
        assert_eq!(text_of(&sink), "keep\n");
    }

    #[test]
    fn delete_first_block_shifts_ranges() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(paragraph(1, "aaa")).unwrap();
        sink.emit(paragraph(2, "bbb")).unwrap();
        sink.emit(paragraph(3, "ccc")).unwrap();
        assert_eq!(text_of(&sink), "aaa\nbbb\nccc\n");

        sink.emit(StreamItem::<Node<(), ()>>::Delete { id: 1 })
            .unwrap();
        assert_eq!(text_of(&sink), "bbb\nccc\n");

        sink.emit(paragraph(3, "updated")).unwrap();
        assert_eq!(text_of(&sink), "bbb\nupdated\n");
    }

    #[test]
    fn heading_renders_text() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(StreamItem::Upsert(Emission {
            id: 1,
            provisional: false,
            node: Node::<(), ()>::Heading {
                level: 1,
                children: vec![Node::Text {
                    text: "Title".into(),
                    meta: (),
                }],
                meta: (),
            },
        }))
        .unwrap();
        assert_eq!(text_of(&sink), "Title\n");
    }

    #[test]
    fn code_block_renders_as_attachment() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(StreamItem::Upsert(Emission {
            id: 1,
            provisional: false,
            node: Node::<(), ()>::CodeBlock {
                info: Some("rust".into()),
                literal: "fn main() {}\n".into(),
                meta: (),
            },
        }))
        .unwrap();
        let text = text_of(&sink);
        // Code blocks are now NSTextAttachment — text contains U+FFFC (object replacement char)
        assert!(
            text.contains('\u{FFFC}'),
            "code block should be an attachment: {text:?}"
        );
    }

    #[test]
    fn list_with_items() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(StreamItem::Upsert(Emission {
            id: 1,
            provisional: false,
            node: Node::<(), ()>::List {
                ordered: false,
                start: None,
                tight: Some(true),
                children: vec![
                    Node::Item {
                        children: vec![Node::Paragraph {
                            children: vec![Node::Text {
                                text: "alpha".into(),
                                meta: (),
                            }],
                            meta: (),
                        }],
                        meta: (),
                    },
                    Node::Item {
                        children: vec![Node::Paragraph {
                            children: vec![Node::Text {
                                text: "beta".into(),
                                meta: (),
                            }],
                            meta: (),
                        }],
                        meta: (),
                    },
                ],
                meta: (),
            },
        }))
        .unwrap();

        let text = text_of(&sink);
        assert!(text.contains("alpha"), "should contain alpha: {text}");
        assert!(text.contains("beta"), "should contain beta: {text}");
        assert!(text.contains('\u{2022}'), "should contain bullet: {text}");
    }

    #[test]
    fn task_item_checkbox() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(StreamItem::Upsert(Emission {
            id: 1,
            provisional: false,
            node: Node::<(), ()>::List {
                ordered: false,
                start: None,
                tight: Some(true),
                children: vec![Node::TaskItem {
                    checked: true,
                    children: vec![Node::Paragraph {
                        children: vec![Node::Text {
                            text: "done".into(),
                            meta: (),
                        }],
                        meta: (),
                    }],
                    meta: (),
                }],
                meta: (),
            },
        }))
        .unwrap();

        let text = text_of(&sink);
        assert!(
            text.contains('\u{2611}'),
            "should contain checked box: {text}"
        );
        assert!(text.contains("done"), "should contain task text: {text}");
    }

    #[test]
    fn inline_emphasis_and_strong() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(StreamItem::Upsert(Emission {
            id: 1,
            provisional: false,
            node: Node::<(), ()>::Paragraph {
                children: vec![
                    Node::Text {
                        text: "normal ".into(),
                        meta: (),
                    },
                    Node::Strong {
                        children: vec![Node::Text {
                            text: "bold".into(),
                            meta: (),
                        }],
                        meta: (),
                    },
                    Node::Text {
                        text: " ".into(),
                        meta: (),
                    },
                    Node::Emphasis {
                        children: vec![Node::Text {
                            text: "italic".into(),
                            meta: (),
                        }],
                        meta: (),
                    },
                ],
                meta: (),
            },
        }))
        .unwrap();
        assert_eq!(text_of(&sink), "normal bold italic\n");
    }

    #[test]
    fn delete_nonexistent_id_is_noop() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(paragraph(1, "hi")).unwrap();
        sink.emit(StreamItem::<Node<(), ()>>::Delete { id: 999 })
            .unwrap();
        assert_eq!(text_of(&sink), "hi\n");
    }

    #[test]
    fn replace_middle_block_preserves_surrounding() {
        let mut sink = TextKitSink::new(test_mtm());
        sink.emit(paragraph(1, "aaa")).unwrap();
        sink.emit(paragraph(2, "bbb")).unwrap();
        sink.emit(paragraph(3, "ccc")).unwrap();
        assert_eq!(text_of(&sink), "aaa\nbbb\nccc\n");

        sink.emit(paragraph(2, "REPLACED")).unwrap();
        assert_eq!(text_of(&sink), "aaa\nREPLACED\nccc\n");
    }
}
