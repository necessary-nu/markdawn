use crate::node::Node;

pub struct InlineMatch<Ext, Meta> {
    pub consumed: usize,
    pub node: Node<Ext, Meta>,
}

pub struct BlockMatch<Ext, Meta> {
    pub consumed_lines: usize,
    pub node: Node<Ext, Meta>,
}

pub trait Extension<Ext, Meta>: Send + Sync {
    /// Called during inline parsing with the remaining input at the current
    /// position. Return `Some(..)` to consume `consumed` bytes and emit a node.
    fn try_inline(&self, _input: &str) -> Option<InlineMatch<Ext, Meta>> {
        None
    }

    /// Called during block parsing with the document's line buffer and the
    /// current line index. Return `Some(..)` to consume `consumed_lines` lines
    /// starting at `start` and emit a block node.
    fn try_block(&self, _lines: &[String], _start: usize) -> Option<BlockMatch<Ext, Meta>> {
        None
    }
}
