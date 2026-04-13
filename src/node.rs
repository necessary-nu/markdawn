pub type CorrelationId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emission<N> {
    pub id: CorrelationId,
    pub provisional: bool,
    pub node: N,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamItem<N> {
    Upsert(Emission<N>),
    Delete { id: CorrelationId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<Ext = (), Meta = ()> {
    Document {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Paragraph {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Heading {
        level: u8,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    BlockQuote {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        tight: Option<bool>,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Item {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    TaskItem {
        checked: bool,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Table {
        alignments: Vec<Option<TableAlignment>>,
        header: Vec<Node<Ext, Meta>>,
        rows: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    TableRow {
        cells: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    TableCell {
        children: Vec<Node<Ext, Meta>>,
        header: bool,
        alignment: Option<TableAlignment>,
        meta: Meta,
    },
    Text {
        text: String,
        meta: Meta,
    },
    Emphasis {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Strong {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Strikethrough {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Link {
        destination: String,
        title: Option<String>,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Image {
        destination: String,
        title: Option<String>,
        alt: String,
        meta: Meta,
    },
    CodeSpan {
        code: String,
        meta: Meta,
    },
    CodeBlock {
        info: Option<String>,
        literal: String,
        meta: Meta,
    },
    Html {
        html: String,
        meta: Meta,
    },
    SoftBreak {
        meta: Meta,
    },
    HardBreak {
        meta: Meta,
    },
    ThematicBreak {
        meta: Meta,
    },
    Custom {
        payload: Ext,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
}
