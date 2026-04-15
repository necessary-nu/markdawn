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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

// gfm[impl blocks-and-inlines.container-and-leaf-blocks-definition]
// gfm[impl container-blocks.definition]
// gfm[impl container-blocks.general-recursive-definition-form]
// gfm[impl container-blocks.syntax-is-defined-recursively]
// gfm[impl container-blocks.generated-from-contents-rather-than-parse-recipe]
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
    // gfm[impl container-blocks.basic-kinds-are-block-quotes-and-list-items]
    BlockQuote {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    // gfm[impl container-blocks.lists-are-meta-containers-for-list-items]
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
    // gfm[impl container-blocks.task-list-items-extension.rendered-marker-becomes-semantic-checkbox]
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
    Subscript {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Superscript {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Highlight {
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
    MathInline {
        math: String,
        meta: Meta,
    },
    MathBlock {
        math: String,
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
    FootnoteRef {
        label: String,
        number: u32,
        ref_num: u32,
        meta: Meta,
    },
    FootnoteDefinition {
        label: String,
        number: u32,
        backref_count: u32,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    FootnoteSection {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Alert {
        kind: AlertKind,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    DefinitionList {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    DefinitionTerm {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    DefinitionDescription {
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
    Custom {
        payload: Ext,
        children: Vec<Node<Ext, Meta>>,
        meta: Meta,
    },
}
