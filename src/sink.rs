use crate::node::StreamItem;

// [spec:mw:def:core.sink.trait]
pub trait Sink<N> {
    type Error;

    fn emit(&mut self, item: StreamItem<N>) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VecSink<N> {
    items: Vec<StreamItem<N>>,
}

impl<N> VecSink<N> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn items(&self) -> &[StreamItem<N>] {
        &self.items
    }

    pub fn into_items(self) -> Vec<StreamItem<N>> {
        self.items
    }
}

// [spec:mw:def:core.sink.vec-sink]
impl<N> Sink<N> for VecSink<N> {
    type Error = core::convert::Infallible;

    fn emit(&mut self, item: StreamItem<N>) -> Result<(), Self::Error> {
        self.items.push(item);
        Ok(())
    }
}
