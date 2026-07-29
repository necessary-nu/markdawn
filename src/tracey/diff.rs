//! Markdown diff helpers used by tracey to render historical rule changes.
//!
//! `diff_markdown` produces a markdown string where removed blocks are wrapped
//! in `~~strike~~` and added blocks in `**bold**`. `diff_markdown_inline` does
//! the same but, when blocks of matching variants line up, falls back to a
//! word-level diff inside the block.

use crate::node::Node;
use crate::node::StreamItem;
use crate::parser::Parser;
use crate::serializer::serialize;
use crate::sink::VecSink;

#[derive(Debug)]
enum DiffOp<T> {
    Equal(T),
    Remove(T),
    Add(T),
}

fn diff_sequences<'a, T: PartialEq>(old: &'a [T], new: &'a [T]) -> Vec<DiffOp<&'a T>> {
    let m = old.len();
    let n = new.len();
    let mut table = vec![vec![0u32; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                table[i][j] = table[i - 1][j - 1] + 1;
            } else {
                table[i][j] = table[i - 1][j].max(table[i][j - 1]);
            }
        }
    }
    let mut ops = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
            ops.push(DiffOp::Equal(&old[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || table[i][j - 1] >= table[i - 1][j]) {
            ops.push(DiffOp::Add(&new[j - 1]));
            j -= 1;
        } else {
            ops.push(DiffOp::Remove(&old[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

fn parse_blocks(source: &str) -> Vec<Node<()>> {
    let mut parser = Parser::<()>::builder().with_all().build();
    let mut sink = VecSink::new();
    if parser.push_str(source, &mut sink).is_err() {
        return Vec::new();
    }
    if parser.finish(&mut sink).is_err() {
        return Vec::new();
    }
    let mut blocks: Vec<(u64, Node<()>)> = Vec::new();
    for item in sink.into_items() {
        match item {
            StreamItem::Upsert(emission) => {
                if let Some(slot) = blocks.iter_mut().find(|(id, _)| *id == emission.id) {
                    slot.1 = emission.node;
                } else {
                    blocks.push((emission.id, emission.node));
                }
            }
            StreamItem::Delete { id } => {
                blocks.retain(|(existing, _)| *existing != id);
            }
        }
    }
    blocks.into_iter().map(|(_, n)| n).collect()
}

fn block_md(block: &Node<()>) -> String {
    serialize(block).trim_end_matches('\n').to_string()
}

fn variant_tag(block: &Node<()>) -> &'static str {
    match block {
        Node::Paragraph { .. } => "paragraph",
        Node::Heading { .. } => "heading",
        Node::BlockQuote { .. } => "blockquote",
        Node::CodeBlock { .. } => "codeblock",
        Node::List { .. } => "list",
        _ => "other",
    }
}

fn wrap_strike(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!("~~{text}~~")
    }
}

fn wrap_strong(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!("**{text}**")
    }
}

fn diff_words(old: &str, new: &str) -> String {
    let old_words: Vec<&str> = old.split_whitespace().collect();
    let new_words: Vec<&str> = new.split_whitespace().collect();
    let ops = diff_sequences(&old_words, &new_words);

    let mut out = String::new();
    let mut removed_run: Vec<&str> = Vec::new();
    let mut added_run: Vec<&str> = Vec::new();

    let flush = |out: &mut String, removed: &mut Vec<&str>, added: &mut Vec<&str>| {
        if !removed.is_empty() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&wrap_strike(&removed.join(" ")));
            removed.clear();
        }
        if !added.is_empty() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&wrap_strong(&added.join(" ")));
            added.clear();
        }
    };

    for op in ops {
        match op {
            DiffOp::Equal(word) => {
                flush(&mut out, &mut removed_run, &mut added_run);
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(word);
            }
            DiffOp::Remove(word) => {
                removed_run.push(*word);
            }
            DiffOp::Add(word) => {
                added_run.push(*word);
            }
        }
    }
    flush(&mut out, &mut removed_run, &mut added_run);
    out
}

/// Block-level markdown diff. Removed blocks are wrapped in `~~~~`, added
/// blocks in `****`. Matching equal blocks are emitted unchanged.
// [spec:mw:sem:traceyext.diff.markdown]
pub fn diff_markdown(old: &str, new: &str) -> String {
    let old_blocks = parse_blocks(old);
    let new_blocks = parse_blocks(new);
    let old_md: Vec<String> = old_blocks.iter().map(block_md).collect();
    let new_md: Vec<String> = new_blocks.iter().map(block_md).collect();
    let ops = diff_sequences(&old_md, &new_md);

    let mut out = String::new();
    for op in ops {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        match op {
            DiffOp::Equal(md) => out.push_str(md),
            DiffOp::Remove(md) => out.push_str(&wrap_strike(md)),
            DiffOp::Add(md) => out.push_str(&wrap_strong(md)),
        }
    }
    out
}

/// Like [`diff_markdown`], but consecutive remove/add pairs of matching block
/// variants get a word-level diff inside the block instead of being wholesale
/// strike/strong.
// [spec:mw:sem:traceyext.diff.markdown]
pub fn diff_markdown_inline(old: &str, new: &str) -> String {
    let old_blocks = parse_blocks(old);
    let new_blocks = parse_blocks(new);
    let old_md: Vec<String> = old_blocks.iter().map(block_md).collect();
    let new_md: Vec<String> = new_blocks.iter().map(block_md).collect();
    let ops = diff_sequences(&old_md, &new_md);

    let mut out = String::new();
    let mut pending_removes: Vec<usize> = Vec::new();

    let push_block = |out: &mut String, text: &str| {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(text);
    };

    let flush_removes = |out: &mut String, pending: &mut Vec<usize>| {
        for idx in pending.drain(..) {
            push_block(out, &wrap_strike(&old_md[idx]));
        }
    };

    let mut i_old = 0usize;
    let mut i_new = 0usize;
    for op in &ops {
        match op {
            DiffOp::Equal(_) => {
                let mut removes_clone = pending_removes.clone();
                pending_removes.clear();
                flush_removes(&mut out, &mut removes_clone);
                push_block(&mut out, &old_md[i_old]);
                i_old += 1;
                i_new += 1;
            }
            DiffOp::Remove(_) => {
                pending_removes.push(i_old);
                i_old += 1;
            }
            DiffOp::Add(_) => {
                let new_idx = i_new;
                i_new += 1;
                let new_block = &new_blocks[new_idx];
                if let Some(pos) = pending_removes.iter().position(|&old_idx| {
                    variant_tag(&old_blocks[old_idx]) == variant_tag(new_block)
                }) {
                    let old_idx = pending_removes.remove(pos);
                    let merged = diff_words(&old_md[old_idx], &new_md[new_idx]);
                    push_block(&mut out, &merged);
                } else {
                    let mut removes_clone = pending_removes.clone();
                    pending_removes.clear();
                    flush_removes(&mut out, &mut removes_clone);
                    push_block(&mut out, &wrap_strong(&new_md[new_idx]));
                }
            }
        }
    }
    let mut tail = pending_removes.clone();
    pending_removes.clear();
    flush_removes(&mut out, &mut tail);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:sem:traceyext.diff.markdown/test]
    #[test]
    fn identical_passes_through() {
        let md = "# Hello\n\nWorld.";
        let out = diff_markdown_inline(md, md);
        assert!(out.contains("Hello"));
        assert!(!out.contains("~~"));
        assert!(!out.contains("**"));
    }

    // [spec:mw:sem:traceyext.diff.markdown/test]
    #[test]
    fn word_change_uses_inline_diff() {
        let old = "The fox is quick.";
        let new = "The fox is fast.";
        let out = diff_markdown_inline(old, new);
        assert!(out.contains("~~quick.~~"));
        assert!(out.contains("**fast.**"));
    }

    // [spec:mw:sem:traceyext.diff.markdown/test]
    #[test]
    fn block_removal() {
        let old = "First paragraph.\n\nSecond paragraph.";
        let new = "First paragraph.";
        let out = diff_markdown_inline(old, new);
        assert!(out.contains("~~Second paragraph.~~"));
    }
}
