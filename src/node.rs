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

/// Byte range of a node in the original markdown source.
///
/// `start..end` is half-open. `start` points at the first byte of the
/// construct (including any leading marker like `#`, `>`, or `\``); `end` is
/// exclusive and includes any trailing newline that belongs to the block.
/// For inline nodes, the span includes the delimiters (e.g. `**bold**` spans
/// both pairs of asterisks).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

// gfm[impl blocks-and-inlines.container-and-leaf-blocks-definition]
// gfm[impl container-blocks.definition]
// gfm[impl container-blocks.general-recursive-definition-form]
// gfm[impl container-blocks.syntax-is-defined-recursively]
// gfm[impl container-blocks.generated-from-contents-rather-than-parse-recipe]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<Ext = ()> {
    Document {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Paragraph {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Heading {
        level: u8,
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    // gfm[impl container-blocks.basic-kinds-are-block-quotes-and-list-items]
    BlockQuote {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    // gfm[impl container-blocks.lists-are-meta-containers-for-list-items]
    List {
        ordered: bool,
        start: Option<u64>,
        tight: Option<bool>,
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Item {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    // gfm[impl container-blocks.task-list-items-extension.rendered-marker-becomes-semantic-checkbox]
    TaskItem {
        checked: bool,
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Table {
        alignments: Vec<Option<TableAlignment>>,
        header: Vec<Node<Ext>>,
        rows: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    TableRow {
        cells: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    TableCell {
        children: Vec<Node<Ext>>,
        header: bool,
        alignment: Option<TableAlignment>,
        source: SourceSpan,
    },
    Text {
        text: String,
        source: SourceSpan,
    },
    Emphasis {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Strong {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Strikethrough {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Subscript {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Superscript {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Highlight {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Link {
        destination: String,
        title: Option<String>,
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Image {
        destination: String,
        title: Option<String>,
        alt: String,
        source: SourceSpan,
    },
    CodeSpan {
        code: String,
        source: SourceSpan,
    },
    CodeBlock {
        info: Option<String>,
        literal: String,
        source: SourceSpan,
    },
    MathInline {
        math: String,
        source: SourceSpan,
    },
    MathBlock {
        math: String,
        source: SourceSpan,
    },
    Html {
        html: String,
        source: SourceSpan,
    },
    SoftBreak {
        source: SourceSpan,
    },
    HardBreak {
        source: SourceSpan,
    },
    ThematicBreak {
        source: SourceSpan,
    },
    FootnoteRef {
        label: String,
        number: u32,
        ref_num: u32,
        source: SourceSpan,
    },
    FootnoteDefinition {
        label: String,
        number: u32,
        backref_count: u32,
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    FootnoteSection {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Alert {
        kind: AlertKind,
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    DefinitionList {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    DefinitionTerm {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    DefinitionDescription {
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
    Custom {
        payload: Ext,
        children: Vec<Node<Ext>>,
        source: SourceSpan,
    },
}

impl<Ext> Node<Ext> {
    /// Visit each direct child of this node in document order.
    ///
    /// For container nodes (`Document`, `Paragraph`, `BlockQuote`, etc.) this
    /// yields the `children` slice. For `Table` it yields header rows then
    /// data rows. Leaf nodes (`Text`, `CodeBlock`, `ThematicBreak`, etc.)
    /// yield nothing.
    pub fn for_each_child(&self, mut f: impl FnMut(&Node<Ext>)) {
        match self {
            Node::Document { children, .. }
            | Node::Paragraph { children, .. }
            | Node::Heading { children, .. }
            | Node::BlockQuote { children, .. }
            | Node::List { children, .. }
            | Node::Item { children, .. }
            | Node::TaskItem { children, .. }
            | Node::TableRow {
                cells: children, ..
            }
            | Node::TableCell { children, .. }
            | Node::Emphasis { children, .. }
            | Node::Strong { children, .. }
            | Node::Strikethrough { children, .. }
            | Node::Subscript { children, .. }
            | Node::Superscript { children, .. }
            | Node::Highlight { children, .. }
            | Node::Link { children, .. }
            | Node::FootnoteDefinition { children, .. }
            | Node::FootnoteSection { children, .. }
            | Node::Alert { children, .. }
            | Node::DefinitionList { children, .. }
            | Node::DefinitionTerm { children, .. }
            | Node::DefinitionDescription { children, .. }
            | Node::Custom { children, .. } => {
                for child in children {
                    f(child);
                }
            }
            Node::Table { header, rows, .. } => {
                for row in header {
                    f(row);
                }
                for row in rows {
                    f(row);
                }
            }
            Node::Text { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::CodeBlock { .. }
            | Node::MathInline { .. }
            | Node::MathBlock { .. }
            | Node::Html { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::ThematicBreak { .. }
            | Node::FootnoteRef { .. } => {}
        }
    }

    /// Mutable variant of [`for_each_child`].
    pub fn for_each_child_mut(&mut self, mut f: impl FnMut(&mut Node<Ext>)) {
        match self {
            Node::Document { children, .. }
            | Node::Paragraph { children, .. }
            | Node::Heading { children, .. }
            | Node::BlockQuote { children, .. }
            | Node::List { children, .. }
            | Node::Item { children, .. }
            | Node::TaskItem { children, .. }
            | Node::TableRow {
                cells: children, ..
            }
            | Node::TableCell { children, .. }
            | Node::Emphasis { children, .. }
            | Node::Strong { children, .. }
            | Node::Strikethrough { children, .. }
            | Node::Subscript { children, .. }
            | Node::Superscript { children, .. }
            | Node::Highlight { children, .. }
            | Node::Link { children, .. }
            | Node::FootnoteDefinition { children, .. }
            | Node::FootnoteSection { children, .. }
            | Node::Alert { children, .. }
            | Node::DefinitionList { children, .. }
            | Node::DefinitionTerm { children, .. }
            | Node::DefinitionDescription { children, .. }
            | Node::Custom { children, .. } => {
                for child in children {
                    f(child);
                }
            }
            Node::Table { header, rows, .. } => {
                for row in header {
                    f(row);
                }
                for row in rows {
                    f(row);
                }
            }
            Node::Text { .. }
            | Node::Image { .. }
            | Node::CodeSpan { .. }
            | Node::CodeBlock { .. }
            | Node::MathInline { .. }
            | Node::MathBlock { .. }
            | Node::Html { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::ThematicBreak { .. }
            | Node::FootnoteRef { .. } => {}
        }
    }

    /// Pre-order walk over this node and all descendants. Convenience for
    /// inspecting an entire tree.
    pub fn walk(&self, f: &mut impl FnMut(&Node<Ext>)) {
        f(self);
        self.for_each_child(|child| child.walk(f));
    }

    /// Mutable variant of [`walk`].
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Node<Ext>)) {
        f(self);
        self.for_each_child_mut(|child| child.walk_mut(f));
    }

    /// Returns a mutable reference to this node's `source` span.
    pub fn source_mut(&mut self) -> &mut SourceSpan {
        match self {
            Node::Document { source, .. }
            | Node::Paragraph { source, .. }
            | Node::Heading { source, .. }
            | Node::BlockQuote { source, .. }
            | Node::List { source, .. }
            | Node::Item { source, .. }
            | Node::TaskItem { source, .. }
            | Node::Table { source, .. }
            | Node::TableRow { source, .. }
            | Node::TableCell { source, .. }
            | Node::Text { source, .. }
            | Node::Emphasis { source, .. }
            | Node::Strong { source, .. }
            | Node::Strikethrough { source, .. }
            | Node::Subscript { source, .. }
            | Node::Superscript { source, .. }
            | Node::Highlight { source, .. }
            | Node::Link { source, .. }
            | Node::Image { source, .. }
            | Node::CodeSpan { source, .. }
            | Node::CodeBlock { source, .. }
            | Node::MathInline { source, .. }
            | Node::MathBlock { source, .. }
            | Node::Html { source, .. }
            | Node::SoftBreak { source, .. }
            | Node::HardBreak { source, .. }
            | Node::ThematicBreak { source, .. }
            | Node::FootnoteRef { source, .. }
            | Node::FootnoteDefinition { source, .. }
            | Node::FootnoteSection { source, .. }
            | Node::Alert { source, .. }
            | Node::DefinitionList { source, .. }
            | Node::DefinitionTerm { source, .. }
            | Node::DefinitionDescription { source, .. }
            | Node::Custom { source, .. } => source,
        }
    }

    /// Returns the source span of this node.
    pub fn source(&self) -> SourceSpan {
        match self {
            Node::Document { source, .. }
            | Node::Paragraph { source, .. }
            | Node::Heading { source, .. }
            | Node::BlockQuote { source, .. }
            | Node::List { source, .. }
            | Node::Item { source, .. }
            | Node::TaskItem { source, .. }
            | Node::Table { source, .. }
            | Node::TableRow { source, .. }
            | Node::TableCell { source, .. }
            | Node::Text { source, .. }
            | Node::Emphasis { source, .. }
            | Node::Strong { source, .. }
            | Node::Strikethrough { source, .. }
            | Node::Subscript { source, .. }
            | Node::Superscript { source, .. }
            | Node::Highlight { source, .. }
            | Node::Link { source, .. }
            | Node::Image { source, .. }
            | Node::CodeSpan { source, .. }
            | Node::CodeBlock { source, .. }
            | Node::MathInline { source, .. }
            | Node::MathBlock { source, .. }
            | Node::Html { source, .. }
            | Node::SoftBreak { source, .. }
            | Node::HardBreak { source, .. }
            | Node::ThematicBreak { source, .. }
            | Node::FootnoteRef { source, .. }
            | Node::FootnoteDefinition { source, .. }
            | Node::FootnoteSection { source, .. }
            | Node::Alert { source, .. }
            | Node::DefinitionList { source, .. }
            | Node::DefinitionTerm { source, .. }
            | Node::DefinitionDescription { source, .. }
            | Node::Custom { source, .. } => *source,
        }
    }
}
