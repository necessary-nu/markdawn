use crate::node::Node;

pub struct InlineExtensionMatch<Ext, Meta> {
    pub consumed: usize,
    pub node: Node<Ext, Meta>,
}

pub trait InlineExtension<Ext, Meta>: Send + Sync {
    fn try_parse(&self, input: &str) -> Option<InlineExtensionMatch<Ext, Meta>>;
}
