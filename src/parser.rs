use crate::error::{Error, StreamError};
use crate::ext::{InlineExtension, InlineExtensionMatch};
use crate::node::{CorrelationId, Emission, Node, StreamItem, TableAlignment};
use crate::sink::Sink;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedBlock<Ext, Meta> {
    node: Node<Ext, Meta>,
    provisional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveBlock<Ext, Meta> {
    id: CorrelationId,
    node: Node<Ext, Meta>,
    provisional: bool,
}

struct ParsedListItem<Ext, Meta> {
    block: ParsedBlock<Ext, Meta>,
    loose: bool,
}

type ReferenceDefinitions = HashMap<String, ReferenceDefinition>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceDefinition {
    destination: String,
    title: Option<String>,
}

pub struct ParserBuilder<Ext = (), Meta = ()> {
    inline_extensions: Vec<Box<dyn InlineExtension<Ext, Meta>>>,
}

impl<Ext, Meta> Default for ParserBuilder<Ext, Meta> {
    fn default() -> Self {
        Self {
            inline_extensions: Vec::new(),
        }
    }
}

impl<Ext, Meta> ParserBuilder<Ext, Meta> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_inline_extension(
        mut self,
        extension: impl InlineExtension<Ext, Meta> + 'static,
    ) -> Self {
        self.inline_extensions.push(Box::new(extension));
        self
    }

    pub fn build(self) -> Parser<Ext, Meta> {
        Parser {
            buffer: String::new(),
            finished: false,
            inline_extensions: self.inline_extensions,
            live_blocks: Vec::new(),
            next_id: 1,
        }
    }
}

pub struct Parser<Ext = (), Meta = ()> {
    buffer: String,
    finished: bool,
    inline_extensions: Vec<Box<dyn InlineExtension<Ext, Meta>>>,
    live_blocks: Vec<LiveBlock<Ext, Meta>>,
    next_id: CorrelationId,
}

impl<Ext, Meta> Default for Parser<Ext, Meta> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Ext, Meta> Parser<Ext, Meta> {
    pub fn new() -> Self {
        Self::builder().build()
    }

    pub fn builder() -> ParserBuilder<Ext, Meta> {
        ParserBuilder::new()
    }

    pub fn push_str<S>(&mut self, chunk: &str, sink: &mut S) -> Result<(), StreamError<S::Error>>
    where
        S: Sink<Node<Ext, Meta>>,
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        if self.finished {
            return Err(Error::AlreadyFinished.into());
        }

        if chunk.is_empty() {
            return Ok(());
        }

        self.buffer.push_str(&normalize_chunk(chunk));
        let blocks = self.parse_blocks(false)?;
        self.emit_blocks(blocks, sink)
    }

    pub fn finish<S>(&mut self, sink: &mut S) -> Result<(), StreamError<S::Error>>
    where
        S: Sink<Node<Ext, Meta>>,
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        if self.finished {
            return Err(Error::AlreadyFinished.into());
        }

        self.finished = true;
        let blocks = self.parse_blocks(true)?;
        self.emit_blocks(blocks, sink)
    }

    fn emit_blocks<S>(
        &mut self,
        blocks: Vec<ParsedBlock<Ext, Meta>>,
        sink: &mut S,
    ) -> Result<(), StreamError<S::Error>>
    where
        S: Sink<Node<Ext, Meta>>,
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let previous = core::mem::take(&mut self.live_blocks);
        let mut next_live = Vec::with_capacity(blocks.len());

        for (index, block) in blocks.into_iter().enumerate() {
            let id = previous
                .get(index)
                .map(|live| live.id)
                .unwrap_or_else(|| self.allocate_id());

            let should_emit = match previous.get(index) {
                Some(live) => live.node != block.node || live.provisional != block.provisional,
                None => true,
            };

            if should_emit {
                sink.emit(StreamItem::Upsert(Emission {
                    id,
                    provisional: block.provisional,
                    node: block.node.clone(),
                }))
                .map_err(StreamError::Sink)?;
            }

            next_live.push(LiveBlock {
                id,
                node: block.node,
                provisional: block.provisional,
            });
        }

        for stale in previous.iter().skip(next_live.len()) {
            sink.emit(StreamItem::Delete { id: stale.id })
                .map_err(StreamError::Sink)?;
        }

        self.live_blocks = next_live;
        Ok(())
    }

    fn allocate_id(&mut self) -> CorrelationId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // gfm[impl blocks-and-inlines.precedence.two-phase-parsing]
    fn parse_blocks(&self, finalizing: bool) -> Result<Vec<ParsedBlock<Ext, Meta>>, Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let split = split_lines(&self.buffer);
        let refs = self.collect_reference_definitions(&split.lines)?;
        self.parse_block_lines(&split.lines, finalizing, split.terminated, &refs)
    }

    // gfm[impl leaf-blocks.link-reference-definitions.may-appear-before-or-after-uses]
    // gfm[impl leaf-blocks.link-reference-definitions.scope-is-the-entire-document]
    fn collect_reference_definitions(&self, lines: &[String]) -> Result<ReferenceDefinitions, Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let mut refs = HashMap::new();
        self.collect_reference_definitions_in_lines(lines, &mut refs)?;
        Ok(refs)
    }

    fn collect_reference_definitions_in_lines(
        &self,
        lines: &[String],
        refs: &mut ReferenceDefinitions,
    ) -> Result<(), Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let mut index = 0;
        while index < lines.len() {
            if lines[index].trim().is_empty() {
                index += 1;
                continue;
            }

            if let Some((next, (label, def))) = parse_reference_definition(lines, index) {
                refs.entry(label).or_insert(def);
                index = next;
                continue;
            }

            if let Some((next, inner_lines)) = collect_block_quote_lines(lines, index) {
                self.collect_reference_definitions_in_lines(&inner_lines, refs)?;
                index = next;
                continue;
            }

            if let Some((next, item_lines)) = collect_list_item_lines(lines, index) {
                for item in item_lines {
                    self.collect_reference_definitions_in_lines(&item, refs)?;
                }
                index = next;
                continue;
            }

            if let Some((next, _)) = self.parse_html_block(lines, index, true, true) {
                index = next;
                continue;
            }

            if let Some((next, _)) = self.parse_fenced_code(lines, index, true) {
                index = next;
                continue;
            }

            if let Some((next, _)) = self.parse_indented_code(lines, index, true, true) {
                index = next;
                continue;
            }

            index += 1;
        }

        Ok(())
    }

    // gfm[impl blocks-and-inlines.precedence.block-indicators-take-precedence-over-inline-indicators]
    fn parse_block_lines(
        &self,
        lines: &[String],
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
    ) -> Result<Vec<ParsedBlock<Ext, Meta>>, Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let mut blocks = Vec::new();
        let mut index = 0;

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                index += 1;
                continue;
            }

            if let Some((next, _)) = parse_reference_definition(lines, index) {
                index = next;
                continue;
            }

            if let Some((next, block)) = self.parse_html_block(lines, index, finalizing, terminated)
            {
                blocks.push(block);
                index = next;
                continue;
            }

            if let Some((next, block)) =
                self.parse_table(lines, index, finalizing, terminated, refs)?
            {
                blocks.push(block);
                index = next;
                continue;
            }

            if let Some((next, block)) =
                self.parse_block_quote(lines, index, finalizing, terminated, refs)
            {
                blocks.push(block?);
                index = next;
                continue;
            }

            if let Some((next, block)) =
                self.parse_list(lines, index, finalizing, terminated, refs)?
            {
                blocks.push(block);
                index = next;
                continue;
            }

            if let Some((next, block)) = self.parse_fenced_code(&lines, index, finalizing) {
                blocks.push(block?);
                index = next;
                continue;
            }

            if let Some((next, block)) =
                self.parse_indented_code(lines, index, finalizing, terminated)
            {
                blocks.push(block);
                index = next;
                continue;
            }

            if let Some(block) = self.parse_atx_heading(&lines[index], refs)? {
                blocks.push(ParsedBlock {
                    node: block,
                    provisional: !finalizing && index + 1 == lines.len() && !terminated,
                });
                index += 1;
                continue;
            }

            if is_thematic_break(&lines[index]) {
                blocks.push(ParsedBlock {
                    node: Node::ThematicBreak {
                        meta: Meta::default(),
                    },
                    provisional: !finalizing && index + 1 == lines.len() && !terminated,
                });
                index += 1;
                continue;
            }

            let (next, block) =
                self.parse_paragraphish(&lines, index, finalizing, terminated, refs)?;
            blocks.push(block);
            index = next;
        }

        Ok(blocks)
    }

    // gfm[impl container-blocks.block-quotes.marker-structure]
    // gfm[impl container-blocks.block-quotes.space-after-marker-is-optional]
    // gfm[impl container-blocks.block-quotes.can-have-initial-or-final-blank-lines]
    fn parse_block_quote(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
    ) -> Option<(usize, Result<ParsedBlock<Ext, Meta>, Error>)>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let mut inner_lines = Vec::new();
        let mut index = start;
        let mut saw_marker = false;

        while index < lines.len() {
            if let Some(stripped) = strip_block_quote_marker(&lines[index]) {
                inner_lines.push(stripped);
                saw_marker = true;
                index += 1;
                continue;
            }

            if saw_marker && lines[index].trim().is_empty() {
                inner_lines.push(String::new());
                index += 1;
                continue;
            }

            break;
        }

        if !saw_marker {
            return None;
        }

        let inner_terminated = if index == lines.len() {
            terminated
        } else {
            true
        };
        Some((
            index,
            self.parse_block_lines(&inner_lines, finalizing, inner_terminated, refs)
                .map(|children| ParsedBlock {
                    provisional: children.iter().any(|child| child.provisional),
                    node: Node::BlockQuote {
                        children: children.into_iter().map(|child| child.node).collect(),
                        meta: Meta::default(),
                    },
                }),
        ))
    }

    // gfm[impl container-blocks.list-items.marker-definition]
    // gfm[impl container-blocks.list-items.type-is-determined-by-marker]
    // gfm[impl container-blocks.lists.loose-list-definition]
    // gfm[impl container-blocks.lists.tight-list-definition]
    fn parse_list(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
    ) -> Result<Option<(usize, ParsedBlock<Ext, Meta>)>, Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let Some(first_marker) = parse_list_marker(&lines[start]) else {
            return Ok(None);
        };

        let mut index = start;
        let mut items: Vec<ParsedListItem<Ext, Meta>> = Vec::new();
        while index < lines.len() {
            let Some(marker) = parse_list_marker(&lines[index]) else {
                break;
            };

            if marker.ordered != first_marker.ordered {
                break;
            }

            let (next, item) =
                self.parse_list_item(lines, index, marker, finalizing, terminated, refs)?;
            items.push(item);
            index = next;
        }

        Ok(Some((
            index,
            ParsedBlock {
                provisional: items.iter().any(|item| item.block.provisional),
                node: Node::List {
                    ordered: first_marker.ordered,
                    start: first_marker.start,
                    tight: Some(is_tight_list(&items)),
                    children: items.into_iter().map(|item| item.block.node).collect(),
                    meta: Meta::default(),
                },
            },
        )))
    }

    // gfm[impl container-blocks.list-items.required-indentation-is-marker-width-plus-padding]
    // gfm[impl container-blocks.list-items.sublists-follow-general-indentation-rules]
    // gfm[impl container-blocks.list-items.lazy-continuation-example]
    // gfm[impl container-blocks.list-items.item-blocks-may-be-separated-by-multiple-blank-lines]
    fn parse_list_item(
        &self,
        lines: &[String],
        start: usize,
        marker: ListMarker,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
    ) -> Result<(usize, ParsedListItem<Ext, Meta>), Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let mut inner_lines = Vec::new();
        inner_lines.push(marker.content);
        let mut index = start + 1;
        let mut saw_blank_separator = false;
        let mut loose = false;

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                inner_lines.push(String::new());
                saw_blank_separator = true;
                index += 1;
                continue;
            }

            if let Some(stripped) = strip_list_continuation(&lines[index], marker.content_indent) {
                if saw_blank_separator {
                    loose = true;
                }
                inner_lines.push(stripped);
                saw_blank_separator = false;
                index += 1;
                continue;
            }

            if !inner_lines
                .last()
                .is_some_and(|line| line.trim().is_empty())
                && is_lazy_list_continuation(&lines[index], lines, index)
            {
                if saw_blank_separator {
                    loose = true;
                }
                inner_lines.push(lines[index].clone());
                saw_blank_separator = false;
                index += 1;
                continue;
            }

            if parse_list_marker(&lines[index]).is_some() {
                break;
            }

            break;
        }

        let inner_terminated = if index == lines.len() {
            terminated
        } else {
            true
        };
        let children = self.parse_block_lines(&inner_lines, finalizing, inner_terminated, refs)?;
        let children = normalize_task_item(children);
        Ok((
            index,
            ParsedListItem {
                loose,
                block: ParsedBlock {
                    provisional: children.iter().any(|child| child.provisional),
                    node: build_item_node(children),
                },
            },
        ))
    }

    // gfm[impl leaf-blocks.indented-code-blocks.definition]
    // gfm[impl leaf-blocks.indented-code-blocks.less-than-four-spaces-ends-block]
    // gfm[impl leaf-blocks.indented-code-blocks.contents-are-literal-text]
    fn parse_indented_code(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
    ) -> Option<(usize, ParsedBlock<Ext, Meta>)>
    where
        Meta: Default + Clone,
        Ext: Clone + PartialEq,
    {
        let first = strip_indented_code_line(&lines[start])?;
        let mut literal = first;
        let mut index = start + 1;

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                literal.push('\n');
                index += 1;
                continue;
            }

            let Some(stripped) = strip_indented_code_line(&lines[index]) else {
                break;
            };
            literal.push('\n');
            literal.push_str(&stripped);
            index += 1;
        }

        Some((
            index,
            ParsedBlock {
                node: Node::CodeBlock {
                    info: None,
                    literal,
                    meta: Meta::default(),
                },
                provisional: index == lines.len() && !finalizing && !terminated,
            },
        ))
    }

    // gfm[impl leaf-blocks.html-blocks.definition]
    // gfm[impl leaf-blocks.html-blocks.ends-at-matching-end-condition-or-container-end]
    // gfm[impl leaf-blocks.html-blocks.type-1-script-pre-style-start-and-end-conditions]
    // gfm[impl leaf-blocks.html-blocks.type-2-comment-start-and-end-conditions]
    // gfm[impl leaf-blocks.html-blocks.type-3-processing-instruction-start-and-end-conditions]
    // gfm[impl leaf-blocks.html-blocks.type-4-declaration-start-and-end-conditions]
    // gfm[impl leaf-blocks.html-blocks.type-5-cdata-start-and-end-conditions]
    // gfm[impl leaf-blocks.html-blocks.type-6-block-tag-start-and-blank-line-end]
    // gfm[impl leaf-blocks.html-blocks.type-7-complete-tag-start-and-blank-line-end]
    // gfm[impl leaf-blocks.html-blocks.types-1-through-5-end-at-matching-end-marker]
    // gfm[impl leaf-blocks.html-blocks.starts-after-up-to-three-spaces-of-indentation]
    fn parse_html_block(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
    ) -> Option<(usize, ParsedBlock<Ext, Meta>)>
    where
        Meta: Default + Clone,
        Ext: Clone + PartialEq,
    {
        let start_info = classify_html_block_start(&lines[start])?;
        let kind = start_info.kind;
        let first_closed = html_block_is_closed_by_line(kind, &lines[start]);
        let is_blank_terminated = matches!(
            kind,
            HtmlBlockKind::Type6BlockTag | HtmlBlockKind::Type7GenericTag
        );

        if first_closed && !is_blank_terminated {
            return Some((
                start + 1,
                ParsedBlock {
                    node: Node::Html {
                        html: lines[start].clone(),
                        meta: Meta::default(),
                    },
                    provisional: false,
                },
            ));
        }

        let mut html = lines[start].clone();
        let mut index = start + 1;

        while index < lines.len() {
            if matches!(
                kind,
                HtmlBlockKind::Type6BlockTag | HtmlBlockKind::Type7GenericTag
            ) && lines[index].trim().is_empty()
            {
                break;
            }

            html.push('\n');
            html.push_str(&lines[index]);

            if html_block_is_closed_by_line(kind, &lines[index]) {
                index += 1;
                break;
            }

            index += 1;
        }

        let closed = if index > start + 1 {
            html_block_is_closed_by_line(kind, &lines[index.saturating_sub(1)])
        } else {
            first_closed
        } || (is_blank_terminated
            && (index < lines.len() || terminated || finalizing));

        Some((
            index,
            ParsedBlock {
                node: Node::Html {
                    html,
                    meta: Meta::default(),
                },
                provisional: !closed && !finalizing,
            },
        ))
    }

    // gfm[impl leaf-blocks.tables-extension.table-definition]
    // gfm[impl leaf-blocks.tables-extension.delimiter-row-structure]
    // gfm[impl leaf-blocks.tables-extension.header-and-delimiter-row-must-have-matching-cell-count]
    // gfm[impl leaf-blocks.tables-extension.table-ends-at-blank-line-or-other-block]
    fn parse_table(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
    ) -> Result<Option<(usize, ParsedBlock<Ext, Meta>)>, Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        if start + 1 >= lines.len() {
            return Ok(None);
        }
        if !lines[start].contains('|') || !lines[start + 1].contains('|') {
            return Ok(None);
        }

        let Some(alignments) = parse_table_delimiter(&lines[start + 1]) else {
            return Ok(None);
        };
        let header_cells = split_table_row(&lines[start]);
        if header_cells.is_empty() || header_cells.len() != alignments.len() {
            return Ok(None);
        }

        let mut rows = Vec::new();
        let mut index = start + 2;
        while index < lines.len() {
            if lines[index].trim().is_empty() || !lines[index].contains('|') {
                break;
            }
            let cells = split_table_row(&lines[index]);
            if cells.len() != alignments.len() {
                break;
            }
            rows.push(Node::TableRow {
                cells: self.parse_table_cells(&cells, false, &alignments, refs)?,
                meta: Meta::default(),
            });
            index += 1;
        }

        Ok(Some((
            index,
            ParsedBlock {
                node: Node::Table {
                    alignments: alignments.clone(),
                    header: vec![Node::TableRow {
                        cells: self.parse_table_cells(&header_cells, true, &alignments, refs)?,
                        meta: Meta::default(),
                    }],
                    rows,
                    meta: Meta::default(),
                },
                provisional: index == lines.len() && !finalizing && !terminated,
            },
        )))
    }

    fn parse_table_cells(
        &self,
        cells: &[String],
        header: bool,
        alignments: &[Option<TableAlignment>],
        refs: &ReferenceDefinitions,
    ) -> Result<Vec<Node<Ext, Meta>>, Error>
    where
        Meta: Default + Clone + PartialEq,
        Ext: Clone + PartialEq,
    {
        let mut out = Vec::with_capacity(cells.len());
        for (index, cell) in cells.iter().enumerate() {
            out.push(Node::TableCell {
                children: self.parse_inline_nodes(cell.trim(), false, refs)?,
                header,
                alignment: alignments.get(index).copied().flatten(),
                meta: Meta::default(),
            });
        }
        Ok(out)
    }

    // gfm[impl leaf-blocks.fenced-code-blocks.code-fence-uses-backticks-or-tildes]
    // gfm[impl leaf-blocks.fenced-code-blocks.opening-fence-may-include-info-string]
    // gfm[impl leaf-blocks.fenced-code-blocks.content-continues-until-matching-closing-fence]
    // gfm[impl leaf-blocks.fenced-code-blocks.unclosed-block-continues-to-end-of-containing-block]
    fn parse_fenced_code(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
    ) -> Option<(usize, Result<ParsedBlock<Ext, Meta>, Error>)>
    where
        Meta: Default + Clone,
        Ext: Clone + PartialEq,
    {
        let opener = parse_fence_opener(&lines[start])?;
        let mut literal = String::new();
        let mut index = start + 1;

        while index < lines.len() {
            if is_fence_closer(&lines[index], opener.marker, opener.count) {
                return Some((
                    index + 1,
                    Ok(ParsedBlock {
                        node: Node::CodeBlock {
                            info: opener.info,
                            literal,
                            meta: Meta::default(),
                        },
                        provisional: false,
                    }),
                ));
            }

            if !literal.is_empty() {
                literal.push('\n');
            }
            literal.push_str(&lines[index]);
            index += 1;
        }

        Some((
            lines.len(),
            Ok(ParsedBlock {
                node: Node::CodeBlock {
                    info: opener.info,
                    literal,
                    meta: Meta::default(),
                },
                provisional: !finalizing,
            }),
        ))
    }

    // gfm[impl leaf-blocks.atx-headings.structure]
    // gfm[impl leaf-blocks.atx-headings.opening-sequence-length-one-to-six]
    // gfm[impl leaf-blocks.atx-headings.opening-sequence-followed-by-space-or-eol]
    // gfm[impl leaf-blocks.atx-headings.contents-parse-as-inline-content]
    fn parse_atx_heading(
        &self,
        line: &str,
        refs: &ReferenceDefinitions,
    ) -> Result<Option<Node<Ext, Meta>>, Error>
    where
        Meta: Default + Clone,
        Ext: Clone + PartialEq,
    {
        let line = trim_up_to_three_spaces(line);
        let mut count = 0;
        for ch in line.chars() {
            if ch == '#' {
                count += 1;
            } else {
                break;
            }
        }

        if count == 0 || count > 6 {
            return Ok(None);
        }

        let rest = &line[count..];
        if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
            return Ok(None);
        }

        let content = strip_atx_content(rest.trim());
        let children = self.parse_inline_nodes(content, false, refs)?;
        Ok(Some(Node::Heading {
            level: count as u8,
            children,
            meta: Meta::default(),
        }))
    }

    // gfm[impl leaf-blocks.paragraphs.structure]
    // gfm[impl leaf-blocks.paragraphs.contents-parse-as-inlines]
    // gfm[impl leaf-blocks.setext-headings.structure]
    // gfm[impl leaf-blocks.setext-headings.contents-parse-preceding-lines-as-inline-content]
    // gfm[impl leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs]
    // gfm[impl leaf-blocks.html-blocks.type-7-cannot-interrupt-paragraphs]
    fn parse_paragraphish(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
    ) -> Result<(usize, ParsedBlock<Ext, Meta>), Error>
    where
        Meta: Default + Clone,
        Ext: Clone + PartialEq,
    {
        let mut collected = vec![lines[start].clone()];
        let mut index = start + 1;

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                break;
            }

            if is_setext_underline(&lines[index]) {
                let text = join_lines(&collected);
                let children = self.parse_inline_nodes(text.trim(), false, refs)?;
                let level = if lines[index].trim_start().starts_with('=') {
                    1
                } else {
                    2
                };
                return Ok((
                    index + 1,
                    ParsedBlock {
                        node: Node::Heading {
                            level,
                            children,
                            meta: Meta::default(),
                        },
                        provisional: false,
                    },
                ));
            }

            if parse_fence_opener(&lines[index]).is_some()
                || self.parse_atx_heading(&lines[index], refs)?.is_some()
                || is_thematic_break(&lines[index])
                || strip_block_quote_marker(&lines[index]).is_some()
                || parse_list_marker(&lines[index]).is_some_and(can_interrupt_paragraph_with_list)
                || strip_indented_code_line(&lines[index]).is_some()
                || classify_html_block_start(&lines[index])
                    .is_some_and(|start| start.can_interrupt_paragraph)
            {
                break;
            }

            collected.push(lines[index].clone());
            index += 1;
        }

        let text = join_lines(&collected);
        let provisional = index == lines.len() && !finalizing && (terminated || !text.is_empty());
        let children = self.parse_inline_nodes(&text, provisional, refs)?;

        Ok((
            index,
            ParsedBlock {
                node: Node::Paragraph {
                    children,
                    meta: Meta::default(),
                },
                provisional,
            },
        ))
    }

    // gfm[impl blocks-and-inlines.precedence.two-phase-parsing]
    fn parse_inline_nodes(
        &self,
        text: &str,
        provisional: bool,
        refs: &ReferenceDefinitions,
    ) -> Result<Vec<Node<Ext, Meta>>, Error>
    where
        Meta: Default + Clone,
        Ext: Clone + PartialEq,
    {
        let mut parser = InlineParser {
            input: text,
            index: 0,
            inline_extensions: &self.inline_extensions,
            meta: Meta::default(),
            provisional,
            refs,
        };
        parser.parse_until(None)
    }
}

struct FenceOpener {
    marker: char,
    count: usize,
    info: Option<String>,
}

#[derive(Debug, Clone)]
struct ListMarker {
    ordered: bool,
    start: Option<u64>,
    content: String,
    content_indent: usize,
}

struct IndentPrefix {
    columns: usize,
    bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HtmlBlockKind {
    Type1ScriptStylePreTextarea,
    Type2Comment,
    Type3ProcessingInstruction,
    Type4Declaration,
    Type5Cdata,
    Type6BlockTag,
    Type7GenericTag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HtmlBlockStart {
    kind: HtmlBlockKind,
    can_interrupt_paragraph: bool,
}

const HTML_BLOCK_TYPE_1_TAGS: &[&str] = &["pre", "script", "style", "textarea"];
const HTML_BLOCK_TYPE_6_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

// gfm[impl preliminaries.characters-and-lines.line-ending-definition]
// gfm[impl preliminaries.insecure-characters.null-character-is-replaced]
// gfm[impl preliminaries.tabs.tabs-not-expanded-in-lines]
fn normalize_chunk(chunk: &str) -> String {
    let mut out = String::with_capacity(chunk.len());
    let mut chars = chunk.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\0' => out.push('\u{FFFD}'),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            other => out.push(other),
        }
    }

    out
}

struct SplitLines {
    lines: Vec<String>,
    terminated: bool,
}

fn split_lines(text: &str) -> SplitLines {
    if text.is_empty() {
        return SplitLines {
            lines: Vec::new(),
            terminated: false,
        };
    }

    let terminated = text.ends_with('\n');
    let mut lines: Vec<String> = text.split('\n').map(ToOwned::to_owned).collect();
    if terminated {
        lines.pop();
    }
    SplitLines { lines, terminated }
}

fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

fn parse_reference_definition(
    lines: &[String],
    start: usize,
) -> Option<(usize, (String, ReferenceDefinition))> {
    let line = trim_up_to_three_spaces(&lines[start]);
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    let label = &rest[..end];
    let after = rest[end + 1..].strip_prefix(':')?.trim();
    if after.is_empty() {
        return None;
    }

    let (destination, title) =
        split_destination_and_title(after).unwrap_or_else(|| (after.to_owned(), None));
    let normalized = normalize_reference_label(label);
    Some((
        start + 1,
        (normalized, ReferenceDefinition { destination, title }),
    ))
}

fn collect_block_quote_lines(lines: &[String], start: usize) -> Option<(usize, Vec<String>)> {
    let mut inner = Vec::new();
    let mut index = start;
    let mut saw = false;

    while index < lines.len() {
        if let Some(stripped) = strip_block_quote_marker(&lines[index]) {
            inner.push(stripped);
            saw = true;
            index += 1;
            continue;
        }
        if saw && lines[index].trim().is_empty() {
            inner.push(String::new());
            index += 1;
            continue;
        }
        break;
    }

    saw.then_some((index, inner))
}

fn collect_list_item_lines(lines: &[String], start: usize) -> Option<(usize, Vec<Vec<String>>)> {
    let first_marker = parse_list_marker(&lines[start])?;
    let mut items = Vec::new();
    let mut index = start;

    while index < lines.len() {
        let Some(marker) = parse_list_marker(&lines[index]) else {
            break;
        };
        if marker.ordered != first_marker.ordered {
            break;
        }

        let mut inner = vec![marker.content];
        index += 1;
        while index < lines.len() {
            if parse_list_marker(&lines[index]).is_some() {
                break;
            }
            if lines[index].trim().is_empty() {
                inner.push(String::new());
                index += 1;
                continue;
            }
            if let Some(stripped) = strip_list_continuation(&lines[index], marker.content_indent) {
                inner.push(stripped);
                index += 1;
                continue;
            }
            break;
        }
        items.push(inner);
    }

    Some((index, items))
}

fn trim_up_to_three_spaces(line: &str) -> &str {
    let mut removed = 0;
    let mut start = 0;
    for (index, ch) in line.char_indices() {
        if removed == 3 || ch != ' ' {
            start = index;
            return &line[start..];
        }
        removed += 1;
        start = index + ch.len_utf8();
    }
    &line[start..]
}

fn strip_atx_content(rest: &str) -> &str {
    let trimmed = rest.trim();
    let mut end = trimmed.len();
    let bytes = trimmed.as_bytes();

    while end > 0 && bytes[end - 1] == b'#' {
        end -= 1;
    }

    if end < trimmed.len() && end > 0 && bytes[end - 1] == b' ' {
        trimmed[..end].trim_end()
    } else {
        trimmed
    }
}

// gfm[impl preliminaries.tabs.tab-after-block-quote-marker-counts-toward-indent]
fn strip_block_quote_marker(line: &str) -> Option<String> {
    let line = trim_up_to_three_spaces(line);
    let rest = line.strip_prefix('>')?;

    let mut absolute_columns = 1;
    let mut consumed_bytes = 0;
    let mut remaining_indent = 0;
    let mut used_optional_padding = false;

    for ch in rest.chars() {
        let width = match ch {
            ' ' | '\t' => indent_width(absolute_columns, ch),
            _ => break,
        };

        if used_optional_padding {
            remaining_indent += width;
        } else {
            remaining_indent += width.saturating_sub(1);
            used_optional_padding = true;
        }

        absolute_columns += width;
        consumed_bytes += ch.len_utf8();
    }

    let mut out = String::new();
    if remaining_indent >= 4 {
        out.push_str(&" ".repeat(remaining_indent));
    }
    out.push_str(&rest[consumed_bytes..]);
    Some(out)
}

// gfm[impl container-blocks.list-items.bullet-marker-definition]
// gfm[impl container-blocks.list-items.ordered-marker-definition]
// gfm[impl container-blocks.list-items.at-least-one-space-required-after-marker]
// gfm[impl container-blocks.list-items.ordered-start-number-is-at-most-nine-digits]
// gfm[impl container-blocks.list-items.ordered-start-number-may-have-leading-zeroes]
// gfm[impl container-blocks.list-items.empty-item-does-not-require-post-marker-spaces]
// gfm[impl preliminaries.tabs.tab-indented-list-continuation-equivalence]
fn parse_list_marker(line: &str) -> Option<ListMarker> {
    let indent = leading_indent(line);
    if indent.columns > 3 {
        return None;
    }

    let trimmed = &line[indent.bytes..];
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    let marker_width = first.len_utf8();

    if matches!(first, '-' | '+' | '*') {
        let rest = &trimmed[first.len_utf8()..];
        let separator = rest.chars().next();
        if matches!(separator, Some(ch) if ch != ' ' && ch != '\t') {
            return None;
        }
        let separator_width = separator.map(char::len_utf8).unwrap_or(0);
        let separator_columns = separator
            .map(|ch| indent_width(marker_width, ch))
            .unwrap_or(0);
        let content = rest[separator_width..].to_owned();
        let content_indent = if content.trim().is_empty() {
            indent.columns + marker_width + 1
        } else {
            indent.columns + marker_width + separator_columns
        };
        return Some(ListMarker {
            ordered: false,
            start: None,
            content,
            content_indent,
        });
    }

    if !first.is_ascii_digit() {
        return None;
    }

    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count > 9 {
        return None;
    }
    let marker_index = digit_count;
    let marker = trimmed[marker_index..].chars().next()?;
    if marker != '.' && marker != ')' {
        return None;
    }

    let after_marker = &trimmed[marker_index + marker.len_utf8()..];
    let separator = after_marker.chars().next();
    if matches!(separator, Some(ch) if ch != ' ' && ch != '\t') {
        return None;
    }

    let start = trimmed[..digit_count].parse().ok()?;
    let separator_width = separator.map(char::len_utf8).unwrap_or(0);
    let base_columns = indent.columns + marker_index + marker.len_utf8();
    let separator_columns = separator
        .map(|ch| indent_width(base_columns, ch))
        .unwrap_or(0);
    let content = after_marker[separator_width..].to_owned();
    let content_indent = if content.trim().is_empty() {
        base_columns + 1
    } else {
        base_columns + separator_columns
    };
    Some(ListMarker {
        ordered: true,
        start: Some(start),
        content,
        content_indent,
    })
}

fn can_interrupt_paragraph_with_list(marker: ListMarker) -> bool {
    !marker.content.trim().is_empty() && (!marker.ordered || marker.start == Some(1))
}

fn strip_list_continuation(line: &str, required_indent: usize) -> Option<String> {
    if line.trim().is_empty() {
        return Some(String::new());
    }

    strip_indent_columns(line, required_indent)
}

// gfm[impl preliminaries.tabs.tab-indented-code-block-equivalence]
fn strip_indented_code_line(line: &str) -> Option<String> {
    strip_indent_columns(line, 4)
}

fn leading_indent(line: &str) -> IndentPrefix {
    let mut columns = 0;
    let mut bytes = 0;
    for ch in line.chars() {
        match ch {
            ' ' => {
                columns += 1;
                bytes += 1;
            }
            '\t' => {
                columns += indent_width(columns, '\t');
                bytes += 1;
            }
            _ => break,
        }
    }
    IndentPrefix { columns, bytes }
}

fn strip_indent_columns(line: &str, columns: usize) -> Option<String> {
    let mut consumed_columns = 0;
    let mut consumed_bytes = 0;

    for ch in line.chars() {
        if consumed_columns >= columns {
            break;
        }
        let width = match ch {
            ' ' => 1,
            '\t' => indent_width(consumed_columns, '\t'),
            _ => return None,
        };
        consumed_columns += width;
        consumed_bytes += ch.len_utf8();
    }

    if consumed_columns < columns {
        return None;
    }

    let mut out = String::new();
    if consumed_columns > columns {
        out.push_str(&" ".repeat(consumed_columns - columns));
    }
    out.push_str(&line[consumed_bytes..]);
    Some(out)
}

fn indent_width(current_columns: usize, ch: char) -> usize {
    match ch {
        ' ' => 1,
        '\t' => 4 - (current_columns % 4),
        _ => 0,
    }
}

fn parse_fence_opener(line: &str) -> Option<FenceOpener> {
    let line = trim_up_to_three_spaces(line);
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let count = line.chars().take_while(|ch| *ch == marker).count();
    if count < 3 {
        return None;
    }

    let rest = &line[count..];
    if marker == '`' && rest.contains('`') {
        return None;
    }

    let info = (!rest.trim().is_empty()).then(|| rest.trim().to_owned());
    Some(FenceOpener {
        marker,
        count,
        info,
    })
}

fn is_fence_closer(line: &str, marker: char, count: usize) -> bool {
    let line = trim_up_to_three_spaces(line);
    let chars = line.chars();
    let run = chars.take_while(|ch| *ch == marker).count();
    run >= count && line[run..].trim().is_empty()
}

fn is_thematic_break(line: &str) -> bool {
    let line = trim_up_to_three_spaces(line);
    let mut marker = None;
    let mut count = 0;

    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            continue;
        }

        match marker {
            None if matches!(ch, '*' | '-' | '_') => {
                marker = Some(ch);
                count += 1;
            }
            Some(expected) if ch == expected => count += 1,
            _ => return false,
        }
    }

    count >= 3
}

fn is_setext_underline(line: &str) -> bool {
    let line = trim_up_to_three_spaces(line).trim();
    if line.is_empty() {
        return false;
    }

    let marker = line.chars().next().unwrap_or(' ');
    if marker != '=' && marker != '-' {
        return false;
    }

    let mut count = 0;
    for ch in line.chars() {
        if ch == marker {
            count += 1;
        } else if ch != ' ' && ch != '\t' {
            return false;
        }
    }

    count >= 1
}

struct InlineParser<'a, Ext, Meta> {
    input: &'a str,
    index: usize,
    inline_extensions: &'a [Box<dyn InlineExtension<Ext, Meta>>],
    refs: &'a ReferenceDefinitions,
    meta: Meta,
    provisional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmphasisMarker {
    Asterisk,
    Underscore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InlinePiece<Ext, Meta> {
    Text(String),
    Node(Node<Ext, Meta>),
    Delimiter {
        marker: EmphasisMarker,
        count: usize,
        can_open: bool,
        can_close: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DelimiterRun {
    marker: EmphasisMarker,
    count: usize,
    can_open: bool,
    can_close: bool,
}

impl<'a, Ext, Meta> InlineParser<'a, Ext, Meta>
where
    Meta: Default + Clone,
    Ext: Clone + PartialEq,
{
    // gfm[impl inlines.inline-parsing-is-sequential]
    // gfm[impl inlines.earlier-inline-match-takes-precedence-in-sequential-parse]
    // gfm[impl inlines.backslash-escapes.any-ascii-punctuation-may-be-escaped]
    // gfm[impl inlines.backslash-escapes.backslash-at-end-line-is-hard-line-break]
    // gfm[impl inlines.backslash-escapes.backslashes-before-other-characters-are-literal]
    // gfm[impl inlines.backslash-escapes.do-not-work-in-code-autolinks-or-raw-html]
    // gfm[impl inlines.backslash-escapes.escaped-backslash-does-not-escape-following-character]
    // gfm[impl inlines.backslash-escapes.escaped-characters-lose-special-meaning]
    // gfm[impl inlines.backslash-escapes.work-in-urls-titles-references-and-info-strings]
    // gfm[impl inlines.code-spans.definition]
    // gfm[impl inlines.code-spans.code-spans-take-precedence-over-emphasis-and-links]
    // gfm[impl inlines.code-spans.code-spans-html-tags-and-autolinks-share-precedence]
    // gfm[impl inlines.hard-line-breaks.two-trailing-spaces-produce-hard-line-break]
    // gfm[impl inlines.hard-line-breaks.backslash-before-line-ending-also-produces-hard-line-break]
    // gfm[impl inlines.hard-line-breaks.hard-line-break-syntax-does-not-work-at-end-of-block]
    // gfm[impl inlines.hard-line-breaks.hard-line-breaks-can-occur-inside-inline-constructs]
    // gfm[impl inlines.hard-line-breaks.hard-line-breaks-do-not-occur-inside-code-spans-or-html-tags]
    // gfm[impl inlines.hard-line-breaks.leading-spaces-on-next-line-are-ignored]
    // gfm[impl inlines.hard-line-breaks.more-than-two-trailing-spaces-also-work]
    // gfm[impl inlines.soft-line-breaks.regular-line-break-produces-softbreak]
    // gfm[impl inlines.soft-line-breaks.edge-spaces-around-softbreak-are-removed]
    // gfm[impl inlines.soft-line-breaks.renderers-may-optionally-promote-softbreaks]
    // gfm[impl inlines.soft-line-breaks.softbreak-may-render-as-line-ending-or-space]
    // gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-requires-matching-delimiters]
    // gfm[impl inlines.emphasis-and-strong-emphasis.strong-emphasis-requires-matching-delimiters]
    // gfm[impl inlines.textual-content.uninterpreted-characters-are-parsed-as-text]
    // gfm[impl inlines.textual-content.internal-spaces-are-preserved]
    // gfm[impl inlines.textual-content.end-tests-marker]
    fn parse_until(&mut self, stop: Option<&str>) -> Result<Vec<Node<Ext, Meta>>, Error> {
        let mut pieces = Vec::new();
        let mut text = String::new();

        while self.index < self.input.len() {
            if let Some(stop_delim) = stop {
                if self.remaining().starts_with(stop_delim) {
                    break;
                }
            }

            if let Some(InlineExtensionMatch { consumed, node }) = self.try_extension() {
                if consumed == 0
                    || self.index + consumed > self.input.len()
                    || !self.input.is_char_boundary(self.index + consumed)
                {
                    return Err(Error::InvalidExtensionMatch);
                }

                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Node(node));
                self.index += consumed;
                continue;
            }

            if self.remaining().starts_with("\\\n") {
                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Node(Node::HardBreak {
                    meta: self.meta.clone(),
                }));
                self.index += 2;
                continue;
            }

            if self.remaining().starts_with('\n') {
                let trailing_spaces = text.chars().rev().take_while(|ch| *ch == ' ').count();
                if trailing_spaces >= 2 {
                    for _ in 0..2 {
                        text.pop();
                    }
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(Node::HardBreak {
                        meta: self.meta.clone(),
                    }));
                } else {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(Node::SoftBreak {
                        meta: self.meta.clone(),
                    }));
                }
                self.index += 1;
                continue;
            }

            if let Some(code) = self.try_code_span() {
                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Node(Node::CodeSpan {
                    code,
                    meta: self.meta.clone(),
                }));
                continue;
            }

            if let Some(node) = self.try_autolink() {
                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Node(node));
                continue;
            }

            if let Some(node) = self.try_autolink_literal() {
                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Node(node));
                continue;
            }

            if let Some(html) = self.try_html_inline() {
                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Node(Node::Html {
                    html,
                    meta: self.meta.clone(),
                }));
                continue;
            }

            if let Some(node) = self.try_link_or_image()? {
                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Node(node));
                continue;
            }

            if let Some(node) = self.try_delimited("~~", false)? {
                flush_text_piece(&mut pieces, &mut text);
                let Node::Emphasis { children, .. } = node else {
                    unreachable!()
                };
                pieces.push(InlinePiece::Node(Node::Strikethrough {
                    children,
                    meta: self.meta.clone(),
                }));
                continue;
            }

            if let Some(run) = self.parse_delimiter_run() {
                flush_text_piece(&mut pieces, &mut text);
                pieces.push(InlinePiece::Delimiter {
                    marker: run.marker,
                    count: run.count,
                    can_open: run.can_open,
                    can_close: run.can_close,
                });
                self.index += run.count;
                continue;
            }

            if self.remaining().starts_with('\\') {
                let next_index = self.index + '\\'.len_utf8();
                if next_index < self.input.len() {
                    let next = self.input[next_index..].chars().next().unwrap_or('\\');
                    if is_escapable(next) {
                        text.push(next);
                        self.index = next_index + next.len_utf8();
                        continue;
                    }
                }
            }

            if let Some((consumed, decoded)) = parse_entity_reference(self.remaining()) {
                text.push_str(&decoded);
                self.index += consumed;
                continue;
            }

            let next = self
                .remaining()
                .chars()
                .next()
                .ok_or(Error::MalformedEventStream)?;
            text.push(next);
            self.index += next.len_utf8();
        }

        flush_text_piece(&mut pieces, &mut text);
        Ok(resolve_emphasis(pieces, &self.meta))
    }

    fn try_extension(&self) -> Option<InlineExtensionMatch<Ext, Meta>> {
        for extension in self.inline_extensions {
            if let Some(candidate) = extension.try_parse(self.remaining()) {
                return Some(candidate);
            }
        }
        None
    }

    // gfm[impl inlines.code-spans.backtick-string-definition]
    // gfm[impl inlines.code-spans.opening-and-closing-backtick-strings-must-have-equal-length]
    // gfm[impl inlines.code-spans.unclosed-backtick-string-is-literal]
    // gfm[impl inlines.code-spans.longer-delimiters-allow-literal-backticks]
    // gfm[impl inlines.code-spans.example]
    // gfm[impl inlines.code-spans.line-endings-become-spaces]
    // gfm[impl inlines.code-spans.line-endings-conversion-example]
    // gfm[impl inlines.code-spans.interior-spaces-are-preserved]
    // gfm[impl inlines.code-spans.single-edge-space-is-trimmed]
    // gfm[impl inlines.code-spans.edge-space-trimming-requires-space-on-both-sides]
    // gfm[impl inlines.code-spans.edge-space-trimming-allows-literal-backticks-at-edges]
    // gfm[impl inlines.code-spans.only-one-space-is-trimmed]
    // gfm[impl inlines.code-spans.only-ascii-space-is-trimmed]
    // gfm[impl inlines.code-spans.all-space-content-is-not-trimmed]
    // gfm[impl inlines.code-spans.backslash-escapes-are-never-needed]
    // gfm[impl inlines.code-spans.backslashes-are-literal]
    // gfm[impl inlines.code-spans.browser-rendering-may-collapse-spaces]
    // gfm[impl inlines.code-spans.code-white-space-pre-wrap]
    // gfm[impl inlines.code-spans.autolink-wins-when-autolink-parses-first]
    // gfm[impl inlines.code-spans.html-tag-wins-when-tag-parses-first]
    // gfm[impl inlines.code-spans.code-span-prevents-link-parse]
    // gfm[impl inlines.code-spans.code-span-wins-in-ambiguous-autolink-like-text]
    fn try_code_span(&mut self) -> Option<String> {
        let remaining = self.remaining();
        if !remaining.starts_with('`') {
            return None;
        }

        let run = remaining.chars().take_while(|ch| *ch == '`').count();
        let closer = "`".repeat(run);
        let search_start = self.index + run;
        let rest = &self.input[search_start..];
        let offset = rest.find(&closer)?;
        let end = search_start + offset;
        let code = self.input[search_start..end].replace('\n', " ");
        self.index = end + closer.len();
        Some(normalize_code_span(&code))
    }

    // gfm[impl inlines.emphasis-and-strong-emphasis.delimiter-run-definition]
    // gfm[impl inlines.emphasis-and-strong-emphasis.delimiter-run-example]
    // gfm[impl inlines.emphasis-and-strong-emphasis.left-flanking-delimiter-run-definition]
    // gfm[impl inlines.emphasis-and-strong-emphasis.right-flanking-delimiter-run-definition]
    // gfm[impl inlines.emphasis-and-strong-emphasis.left-right-flanking-terminology-note]
    // gfm[impl inlines.emphasis-and-strong-emphasis.single-asterisk-can-open-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.single-asterisk-can-close-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.single-underscore-can-open-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.single-underscore-can-close-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-can-open-strong-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-can-close-strong-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-can-open-strong-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-can-close-strong-emphasis]
    // gfm[impl inlines.emphasis-and-strong-emphasis.underscore-can-open-when-punctuation-precedes]
    // gfm[impl inlines.emphasis-and-strong-emphasis.underscore-can-close-when-punctuation-follows]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-can-open-when-punctuation-precedes]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-can-close-when-punctuation-follows]
    // gfm[impl inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-when-followed-by-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-after-alphanumeric-before-punctuation]
    // gfm[impl inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-punctuation-before-alphanumeric]
    // gfm[impl inlines.emphasis-and-strong-emphasis.asterisk-closing-restriction-example]
    // gfm[impl inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-alphanumeric-before-punctuation]
    // gfm[impl inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-punctuation-before-alphanumeric]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-alphanumeric-before-punctuation]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-punctuation-before-alphanumeric]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-closing-restriction-examples]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-asterisk-closing-cannot-fall-back-to-single-asterisk-rule]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-whitespace]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-alphanumeric-before-punctuation]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-punctuation-before-alphanumeric]
    // gfm[impl inlines.emphasis-and-strong-emphasis.double-underscore-closing-restriction-example]
    // gfm[impl inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-closing]
    // gfm[impl inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-double-underscore]
    // gfm[impl inlines.emphasis-and-strong-emphasis.right-flanking-underscore-does-not-emphasize]
    // gfm[impl inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-allowed]
    // gfm[impl inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-disallowed]
    // gfm[impl inlines.emphasis-and-strong-emphasis.intraword-double-underscore-closing-is-forbidden]
    // gfm[impl inlines.emphasis-and-strong-emphasis.many-implementations-restrict-intraword-underscore]
    // gfm[impl inlines.emphasis-and-strong-emphasis.unicode-nonbreaking-space-counts-as-whitespace]
    fn parse_delimiter_run(&self) -> Option<DelimiterRun> {
        let remaining = self.remaining();
        let marker = match remaining.chars().next()? {
            '*' => EmphasisMarker::Asterisk,
            '_' => EmphasisMarker::Underscore,
            _ => return None,
        };
        let raw = match marker {
            EmphasisMarker::Asterisk => '*',
            EmphasisMarker::Underscore => '_',
        };
        let count = remaining.chars().take_while(|ch| *ch == raw).count();
        let prev = char_before(self.input, self.index);
        let next = char_after(self.input, self.index + count);
        let left_flanking = is_left_flanking(prev, next);
        let right_flanking = is_right_flanking(prev, next);
        let prev_is_punct = is_unicode_punctuation(prev);
        let next_is_punct = is_unicode_punctuation(next);

        let (can_open, can_close) = match marker {
            EmphasisMarker::Asterisk => (left_flanking, right_flanking),
            EmphasisMarker::Underscore => (
                left_flanking && (!right_flanking || prev_is_punct),
                right_flanking && (!left_flanking || next_is_punct),
            ),
        };

        Some(DelimiterRun {
            marker,
            count,
            can_open,
            can_close,
        })
    }

    // gfm[impl inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
    // gfm[impl inlines.strikethrough-extension.new-paragraph-ends-strikethrough-parse]
    // gfm[impl inlines.strikethrough-extension.extension-wrapper]
    // gfm[impl inlines.strikethrough-extension.gfm-enables-strikethrough-extension]
    fn try_delimited(
        &mut self,
        delim: &str,
        strong: bool,
    ) -> Result<Option<Node<Ext, Meta>>, Error> {
        let remaining = self.remaining();
        if !remaining.starts_with(delim) {
            return Ok(None);
        }

        if delim == "~~" && !is_valid_tilde_opener(self.input, self.index, delim.len()) {
            return Ok(None);
        }

        let search_from = self.index + delim.len();
        let Some(inner_end) = find_delimited_closer(self.input, search_from, delim) else {
            return Ok(None);
        };
        let inner_start = search_from;
        let saved = self.index;
        let mut inner = InlineParser {
            input: &self.input[inner_start..inner_end],
            index: 0,
            inline_extensions: self.inline_extensions,
            refs: self.refs,
            meta: self.meta.clone(),
            provisional: self.provisional,
        };
        let children = inner.parse_until(None)?;
        self.index = inner_end + delim.len();

        if strong {
            Ok(Some(Node::Strong {
                children,
                meta: self.meta.clone(),
            }))
        } else if self.index != saved + delim.len() {
            Ok(Some(Node::Emphasis {
                children,
                meta: self.meta.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.index..]
    }

    // gfm[impl inlines.links.definition]
    // gfm[impl inlines.links.inline-link-definition]
    // gfm[impl inlines.links.inline-link-example]
    // gfm[impl inlines.links.inline-link-text-comes-from-link-text-inlines]
    // gfm[impl inlines.links.inline-link-uri-comes-from-destination-with-delimiters-removed]
    // gfm[impl inlines.links.inline-link-title-comes-from-title-with-delimiters-removed]
    // gfm[impl inlines.links.inline-and-reference-links-are-the-two-basic-kinds]
    // gfm[impl inlines.links.inline-links-take-precedence-over-reference-links]
    // gfm[impl inlines.links.full-reference-link-definition]
    // gfm[impl inlines.links.full-reference-link-example]
    // gfm[impl inlines.links.full-and-collapsed-references-take-precedence-over-shortcuts]
    // gfm[impl inlines.links.collapsed-reference-link-definition]
    // gfm[impl inlines.links.collapsed-reference-link-is-equivalent-to-full-link-with-same-label]
    // gfm[impl inlines.links.collapsed-reference-link-labels-are-case-insensitive]
    // gfm[impl inlines.links.collapsed-reference-link-text-comes-from-first-label-inlines]
    // gfm[impl inlines.links.no-whitespace-allowed-between-collapsed-reference-brackets]
    // gfm[impl inlines.links.shortcut-reference-link-definition]
    // gfm[impl inlines.links.shortcut-reference-link-is-equivalent-to-collapsed-reference-link]
    // gfm[impl inlines.links.shortcut-reference-link-labels-are-case-insensitive]
    // gfm[impl inlines.links.shortcut-reference-link-text-comes-from-label-inlines]
    // gfm[impl inlines.links.shortcut-reference-does-not-parse-when-followed-by-link-label]
    // gfm[impl inlines.links.shortcut-reference-links-make-space-before-label-too-dangerous]
    // gfm[impl inlines.links.space-after-shortcut-reference-link-is-preserved]
    // gfm[impl inlines.links.reference-links-have-three-kinds]
    // gfm[impl inlines.links.reference-link-text-allows-balanced-or-escaped-brackets]
    // gfm[impl inlines.links.reference-link-text-cannot-contain-links]
    // gfm[impl inlines.links.reference-link-text-comes-from-first-label-inlines]
    // gfm[impl inlines.links.reference-link-text-follows-inline-link-text-rules]
    // gfm[impl inlines.links.reference-link-text-may-contain-inline-content]
    // gfm[impl inlines.links.reference-link-text-grouping-takes-precedence-over-emphasis]
    // gfm[impl inlines.links.reference-link-may-parse-later-brackets-as-separate-text]
    // gfm[impl inlines.links.reference-links-lose-to-html-tags-code-spans-and-autolinks]
    // gfm[impl inlines.links.first-matching-reference-definition-is-used]
    // gfm[impl inlines.links.first-reference-definition-wins-example]
    // gfm[impl inlines.links.defined-middle-label-makes-earlier-reference-parse]
    // gfm[impl inlines.links.nested-reference-link-examples-produce-shortcuts]
    // gfm[impl inlines.links.escaping-opening-bracket-prevents-reference-link]
    // gfm[impl inlines.links.link-text-definition]
    // gfm[impl inlines.links.link-text-may-contain-inline-content]
    // gfm[impl inlines.links.link-text-may-contain-balanced-brackets-but-not-unbalanced-ones]
    // gfm[impl inlines.links.link-text-allows-only-escaped-or-balanced-brackets]
    // gfm[impl inlines.links.link-text-cannot-contain-links]
    // gfm[impl inlines.links.link-text-brackets-bind-more-tightly-than-emphasis]
    // gfm[impl inlines.links.links-may-not-contain-other-links-at-any-level-nesting]
    // gfm[impl inlines.links.brackets-aren-t-part-links-do-not-take-precedence]
    // gfm[impl inlines.links.code-autolink-and-html-bind-tighter-than-link-text]
    // gfm[impl inlines.links.cases-illustrate-precedence-html-tags-code-spans-autolinks-over-link]
    // gfm[impl inlines.links.cases-illustrate-precedence-link-text-grouping-over-emphasis-grouping]
    // gfm[impl inlines.links.destination-and-title-may-both-be-omitted]
    // gfm[impl inlines.links.no-whitespace-allowed-between-link-text-and-opening-parenthesis]
    // gfm[impl inlines.links.no-whitespace-allowed-between-link-text-and-link-label]
    // gfm[impl inlines.links.no-space-before-link-label-departs-from-original-markdown]
    // gfm[impl inlines.links.whitespace-is-allowed-inside-inline-link-delimiters]
    // gfm[impl inlines.links.matching-uses-normalized-strings-not-parsed-inline-content]
    // gfm[impl inlines.links.markdown-pl-title-behavior]
    // gfm[impl inlines.images.definition]
    // gfm[impl inlines.images.html-rendering-should-use-plain-string-content-for-alt]
    // gfm[impl inlines.images.image-description-follows-link-text-rules-with-two-differences]
    // gfm[impl inlines.images.image-description-contains-inline-elements]
    // gfm[impl inlines.images.escaping-bang-produces-literal-bang-followed-by-link]
    // gfm[impl inlines.images.escaping-opening-bracket-after-bang-produces-literal-bracketed-text]
    // gfm[impl inlines.images.no-whitespace-allowed-between-reference-image-brackets]
    // gfm[impl inlines.images.reference-image-labels-are-case-insensitive]
    // gfm[impl inlines.images.reference-image-labels-cannot-contain-unescaped-brackets]
    // gfm[impl inlines.images.shortcut-image-labels-are-case-insensitive]
    fn try_link_or_image(&mut self) -> Result<Option<Node<Ext, Meta>>, Error> {
        let remaining = self.remaining();
        let image = remaining.starts_with("![");
        let start_len = if image {
            2
        } else if remaining.starts_with('[') {
            1
        } else {
            return Ok(None);
        };

        let label_start = self.index + start_len;
        let Some(label_end) = find_closing_bracket(self.input, label_start) else {
            return Ok(None);
        };
        let label = &self.input[label_start..label_end];
        let after = &self.input[label_end + 1..];

        if let Some(after_open) = after.strip_prefix('(') {
            let Some((consumed, destination, title)) = parse_inline_link_target(after_open) else {
                return Ok(None);
            };

            let children = parse_inline_children(
                label,
                self.inline_extensions,
                self.refs,
                self.meta.clone(),
                self.provisional,
            )?;
            self.index = label_end + 2 + consumed;
            return Ok(Some(build_link_or_image(
                image,
                destination,
                title,
                children,
                self.meta.clone(),
            )));
        }

        let mut consumed_after = 0;
        let target_label = if let Some(rest) = after.strip_prefix("[]") {
            consumed_after = after.len() - rest.len();
            label.to_owned()
        } else if after.strip_prefix('[').is_some() {
            let target_start = label_end + 2;
            let Some(target_end) = find_closing_bracket(self.input, target_start) else {
                return Ok(None);
            };
            consumed_after = target_end - label_end;
            self.input[target_start..target_end].to_owned()
        } else {
            label.to_owned()
        };

        let normalized = normalize_reference_label(&target_label);
        let Some(def) = self.refs.get(&normalized) else {
            return Ok(None);
        };

        let children = parse_inline_children(
            label,
            self.inline_extensions,
            self.refs,
            self.meta.clone(),
            self.provisional,
        )?;
        self.index = label_end + 1 + consumed_after;
        Ok(Some(build_link_or_image(
            image,
            def.destination.clone(),
            def.title.clone(),
            children,
            self.meta.clone(),
        )))
    }

    // gfm[impl inlines.raw-html.html-like-tag-text-is-parsed-as-raw-html]
    // gfm[impl inlines.raw-html.html-tag-definition]
    // gfm[impl inlines.raw-html.tag-grammar-follows]
    // gfm[impl inlines.raw-html.extension-wrapper]
    // gfm[impl inlines.raw-html.simple-open-tag-examples]
    // gfm[impl inlines.raw-html.custom-tag-names-are-allowed]
    // gfm[impl inlines.raw-html.custom-tag-and-attribute-names-are-allowed]
    // gfm[impl inlines.raw-html.closing-tags-cannot-have-attributes]
    // gfm[impl inlines.raw-html.illegal-tag-and-attribute-syntax-is-not-parsed-as-html]
    // gfm[impl inlines.raw-html.html-comment-definition]
    // gfm[impl inlines.raw-html.processing-instruction-definition]
    // gfm[impl inlines.raw-html.declaration-definition]
    // gfm[impl inlines.raw-html.cdata-section-definition]
    // gfm[impl inlines.raw-html.backslash-escapes-do-not-work-in-html-attributes]
    // gfm[impl inlines.raw-html.entity-and-numeric-references-are-preserved-in-html-attributes]
    // gfm[impl inlines.raw-html.attribute-value-definition]
    // TODO: disallowed raw HTML extension is not yet implemented
    // gfm[impl inlines.disallowed-raw-html-extension.disallowed-tags]
    // gfm[impl inlines.disallowed-raw-html-extension.filtering-replaces-leading-angle-bracket]
    // gfm[impl inlines.disallowed-raw-html-extension.disallowed-tags-change-html-parsing]
    // gfm[impl inlines.disallowed-raw-html-extension.all-other-html-tags-are-left-untouched]
    // gfm[impl inlines.disallowed-raw-html-extension.extension-wrapper]
    // gfm[impl inlines.disallowed-raw-html-extension.gfm-enables-tagfilter-extension]
    fn try_html_inline(&mut self) -> Option<String> {
        let remaining = self.remaining();
        if !remaining.starts_with('<') {
            return None;
        }

        let end = if remaining.starts_with("<!--") {
            remaining.find("-->").map(|idx| idx + 3)?
        } else if remaining.starts_with("<?") {
            remaining.find("?>").map(|idx| idx + 2)?
        } else if remaining.starts_with("<!") {
            remaining.find('>').map(|idx| idx + 1)?
        } else {
            let close = remaining.find('>')?;
            let body = &remaining[1..close];
            if body.is_empty() || !looks_like_inline_tag(body) {
                return None;
            }
            close + 1
        };

        let html = remaining[..end].to_owned();
        self.index += end;
        Some(html)
    }

    // gfm[impl inlines.autolinks.definition]
    // gfm[impl inlines.autolinks.uri-autolink-definition]
    // gfm[impl inlines.autolinks.email-autolink-definition]
    // gfm[impl inlines.autolinks.absolute-uri-definition]
    // gfm[impl inlines.autolinks.scheme-definition]
    // gfm[impl inlines.autolinks.autolink-schemes-are-case-insensitive]
    // gfm[impl inlines.autolinks.autolinks-cannot-contain-spaces]
    // gfm[impl inlines.autolinks.backslash-escapes-do-not-work-inside-autolinks]
    // gfm[impl inlines.autolinks.backslash-escapes-do-not-work-inside-email-autolinks]
    // gfm[impl inlines.autolinks.email-address-definition]
    // gfm[impl inlines.autolinks.email-address-regex]
    // gfm[impl inlines.autolinks.extension-wrapper]
    // gfm[impl inlines.autolinks.invalid-autolink-example]
    // gfm[impl inlines.autolinks.spec-accepts-some-absolute-uris-that-are-not-valid-uris]
    // gfm[impl inlines.autolinks.valid-email-autolink-example]
    // gfm[impl inlines.autolinks.valid-uri-autolink-example]
    fn try_autolink(&mut self) -> Option<Node<Ext, Meta>> {
        let remaining = self.remaining();
        if !remaining.starts_with('<') {
            return None;
        }
        let close = remaining.find('>')?;
        let body = &remaining[1..close];
        if looks_like_url(body) {
            self.index += close + 1;
            return Some(Node::Link {
                destination: body.to_owned(),
                title: None,
                children: vec![Node::Text {
                    text: body.to_owned(),
                    meta: self.meta.clone(),
                }],
                meta: self.meta.clone(),
            });
        }
        if looks_like_email(body) {
            self.index += close + 1;
            let destination = format!("mailto:{body}");
            return Some(Node::Link {
                destination,
                title: None,
                children: vec![Node::Text {
                    text: body.to_owned(),
                    meta: self.meta.clone(),
                }],
                meta: self.meta.clone(),
            });
        }
        None
    }

    // gfm[impl inlines.autolinks-extension.definition]
    // gfm[impl inlines.autolinks-extension.extended-url-autolink-definition]
    // gfm[impl inlines.autolinks-extension.extended-email-autolink-definition]
    // gfm[impl inlines.autolinks-extension.extended-www-autolink-definition]
    // gfm[impl inlines.autolinks-extension.extended-www-autolinks-insert-http-scheme]
    // gfm[impl inlines.autolinks-extension.extended-www-autolinks-may-have-path-after-domain]
    // gfm[impl inlines.autolinks-extension.extended-email-autolink-rules]
    // gfm[impl inlines.autolinks-extension.extended-email-autolinks-insert-mailto-scheme]
    // gfm[impl inlines.autolinks-extension.extended-autolink-path-validation]
    // gfm[impl inlines.autolinks-extension.gfm-enables-extended-autolinks]
    // gfm[impl inlines.autolinks-extension.extension-wrapper]
    // gfm[impl inlines.autolinks-extension.left-angle-bracket-ends-autolink]
    // gfm[impl inlines.autolinks-extension.trailing-punctuation-is-excluded-from-autolinks]
    // gfm[impl inlines.autolinks-extension.trailing-semicolon-is-excluded-when-it-looks-like-entity]
    // gfm[impl inlines.autolinks-extension.trailing-parenthesis-check-applies-only-at-link-end]
    // gfm[impl inlines.autolinks-extension.unmatched-trailing-right-parentheses-are-excluded]
    // gfm[impl inlines.autolinks-extension.dot-dash-and-underscore-rules-around-at-sign]
    // gfm[impl inlines.autolinks-extension.plus-is-allowed-before-but-not-after-at-sign]
    fn try_autolink_literal(&mut self) -> Option<Node<Ext, Meta>> {
        let remaining = self.remaining();
        for prefix in ["https://", "http://"] {
            if let Some(rest) = remaining.strip_prefix(prefix) {
                let end = rest
                    .find(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '>'))
                    .unwrap_or(rest.len());
                let body = format!("{prefix}{}", &rest[..end]);
                self.index += body.len();
                return Some(Node::Link {
                    destination: body.clone(),
                    title: None,
                    children: vec![Node::Text {
                        text: body,
                        meta: self.meta.clone(),
                    }],
                    meta: self.meta.clone(),
                });
            }
        }

        let end = remaining
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '>'))
            .unwrap_or(remaining.len());
        let candidate = &remaining[..end];
        if looks_like_email(candidate) {
            self.index += candidate.len();
            return Some(Node::Link {
                destination: format!("mailto:{candidate}"),
                title: None,
                children: vec![Node::Text {
                    text: candidate.to_owned(),
                    meta: self.meta.clone(),
                }],
                meta: self.meta.clone(),
            });
        }

        None
    }
}

fn flush_text_piece<Ext, Meta>(pieces: &mut Vec<InlinePiece<Ext, Meta>>, text: &mut String) {
    if text.is_empty() {
        return;
    }
    pieces.push(InlinePiece::Text(core::mem::take(text)));
}

// gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-requires-matching-delimiters]
// gfm[impl inlines.emphasis-and-strong-emphasis.strong-emphasis-requires-matching-delimiters]
// gfm[impl inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-permitted]
// gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-is-not-allowed-inside-words]
// gfm[impl inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-permitted]
// gfm[impl inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-forbidden]
// gfm[impl inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-consecutive-emphasis]
// gfm[impl inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-produces-nested-strong]
// gfm[impl inlines.emphasis-and-strong-emphasis.both-multiples-of-three-allow-emphasis-match]
// gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-allows-indefinite-nesting]
// gfm[impl inlines.emphasis-and-strong-emphasis.strong-emphasis-allows-indefinite-nesting]
// gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-emphasis]
// gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-strong]
// gfm[impl inlines.emphasis-and-strong-emphasis.strong-can-nest-inside-strong-with-same-delimiter]
// gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-delimiters-must-match]
// gfm[impl inlines.emphasis-and-strong-emphasis.emphasis-may-contain-any-nonempty-inlines]
// gfm[impl inlines.emphasis-and-strong-emphasis.strong-emphasis-may-contain-any-nonempty-inlines]
// gfm[impl inlines.emphasis-and-strong-emphasis.empty-asterisk-emphasis-is-not-allowed]
// gfm[impl inlines.emphasis-and-strong-emphasis.empty-underscore-emphasis-is-not-allowed]
// gfm[impl inlines.emphasis-and-strong-emphasis.is-emphasis-within-emphasis]
// gfm[impl inlines.emphasis-and-strong-emphasis.rule-11-puts-excess-asterisks-outside-emphasis]
// gfm[impl inlines.emphasis-and-strong-emphasis.rule-12-puts-excess-underscores-outside-emphasis]
// gfm[impl inlines.emphasis-and-strong-emphasis.rule-13-applies-to-long-delimiter-runs]
// gfm[impl inlines.emphasis-and-strong-emphasis.rule-13-requires-different-directly-nested-delimiters]
// gfm[impl inlines.emphasis-and-strong-emphasis.rules-capture-patterns-without-backtracking]
// gfm[impl inlines.emphasis-and-strong-emphasis.rules-illustrated-by-examples]
// gfm[impl inlines.emphasis-and-strong-emphasis.less-common-but-useful-emphasis-patterns]
// gfm[impl inlines.emphasis-and-strong-emphasis.ambiguity-resolution-principles]
// gfm[impl inlines.emphasis-and-strong-emphasis.original-markdown-description]
// gfm[impl inlines.emphasis-and-strong-emphasis.original-markdown-description-quoted]
// gfm[impl inlines.emphasis-and-strong-emphasis.original-rules-leave-nesting-underdetermined]
// gfm[impl inlines.emphasis-and-strong-emphasis.extension-wrapper]
// gfm[impl inlines.emphasis-and-strong-emphasis.literal-asterisk-cannot-edge-asterisk-emphasis]
// gfm[impl inlines.emphasis-and-strong-emphasis.literal-underscore-cannot-edge-underscore-delimiters]
fn resolve_emphasis<Ext, Meta>(
    mut pieces: Vec<InlinePiece<Ext, Meta>>,
    meta: &Meta,
) -> Vec<Node<Ext, Meta>>
where
    Meta: Clone,
    Ext: Clone + PartialEq,
{
    loop {
        let mut changed = false;

        'outer: for closer_index in 0..pieces.len() {
            let (marker, closer_count, can_close, can_open) = match &pieces[closer_index] {
                InlinePiece::Delimiter {
                    marker,
                    count,
                    can_open,
                    can_close,
                } => (*marker, *count, *can_close, *can_open),
                _ => continue,
            };
            if !can_close || closer_count == 0 {
                continue;
            }

            for opener_index in (0..closer_index).rev() {
                let (open_marker, opener_count, opener_can_open, opener_can_close) =
                    match &pieces[opener_index] {
                        InlinePiece::Delimiter {
                            marker,
                            count,
                            can_open,
                            can_close,
                        } => (*marker, *count, *can_open, *can_close),
                        _ => continue,
                    };

                if !opener_can_open || opener_count == 0 || open_marker != marker {
                    continue;
                }
                if multiple_of_three_blocked(opener_count, closer_count, opener_can_close, can_open)
                {
                    continue;
                }

                let used = if opener_count >= 2 && closer_count >= 2 {
                    2
                } else {
                    1
                };
                let inner = pieces
                    .drain(opener_index + 1..closer_index)
                    .collect::<Vec<_>>();
                let closer_piece = pieces.remove(opener_index + 1);
                let opener_piece = pieces.remove(opener_index);

                let (opener_rem, closer_rem) = match (opener_piece, closer_piece) {
                    (
                        InlinePiece::Delimiter { count: oc, .. },
                        InlinePiece::Delimiter { count: cc, .. },
                    ) => (oc - used, cc - used),
                    _ => unreachable!(),
                };

                let children = materialize_inline_pieces(inner, meta);
                let wrapped = if used == 2 {
                    Node::Strong {
                        children,
                        meta: meta.clone(),
                    }
                } else {
                    Node::Emphasis {
                        children,
                        meta: meta.clone(),
                    }
                };

                let mut replacement = Vec::new();
                if opener_rem > 0 {
                    replacement.push(InlinePiece::Delimiter {
                        marker,
                        count: opener_rem,
                        can_open: opener_can_open,
                        can_close: opener_can_close,
                    });
                }
                replacement.push(InlinePiece::Node(wrapped));
                if closer_rem > 0 {
                    replacement.push(InlinePiece::Delimiter {
                        marker,
                        count: closer_rem,
                        can_open,
                        can_close,
                    });
                }

                pieces.splice(opener_index..opener_index, replacement);
                changed = true;
                break 'outer;
            }
        }

        if !changed {
            break;
        }
    }

    materialize_inline_pieces(pieces, meta)
}

fn materialize_inline_pieces<Ext, Meta>(
    pieces: Vec<InlinePiece<Ext, Meta>>,
    meta: &Meta,
) -> Vec<Node<Ext, Meta>>
where
    Meta: Clone,
{
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            InlinePiece::Text(text) if !text.is_empty() => push_or_merge_text(&mut out, text, meta),
            InlinePiece::Node(node) => out.push(node),
            InlinePiece::Delimiter { marker, count, .. } if count > 0 => {
                let ch = match marker {
                    EmphasisMarker::Asterisk => '*',
                    EmphasisMarker::Underscore => '_',
                };
                push_or_merge_text(&mut out, ch.to_string().repeat(count), meta);
            }
            _ => {}
        }
    }
    out
}

fn push_or_merge_text<Ext, Meta>(out: &mut Vec<Node<Ext, Meta>>, text: String, meta: &Meta)
where
    Meta: Clone,
{
    if let Some(Node::Text { text: existing, .. }) = out.last_mut() {
        existing.push_str(&text);
    } else {
        out.push(Node::Text {
            text,
            meta: meta.clone(),
        });
    }
}

fn char_before(input: &str, index: usize) -> Option<char> {
    input[..index].chars().next_back()
}

fn char_after(input: &str, index: usize) -> Option<char> {
    input.get(index..)?.chars().next()
}

fn is_left_flanking(prev: Option<char>, next: Option<char>) -> bool {
    let next_whitespace = next.is_none_or(char::is_whitespace);
    let next_punct = is_unicode_punctuation(next);
    let prev_whitespace = prev.is_none_or(char::is_whitespace);
    let prev_punct = is_unicode_punctuation(prev);

    !next_whitespace && (!next_punct || prev_whitespace || prev_punct)
}

fn is_right_flanking(prev: Option<char>, next: Option<char>) -> bool {
    let prev_whitespace = prev.is_none_or(char::is_whitespace);
    let prev_punct = is_unicode_punctuation(prev);
    let next_whitespace = next.is_none_or(char::is_whitespace);
    let next_punct = is_unicode_punctuation(next);

    !prev_whitespace && (!prev_punct || next_whitespace || next_punct)
}

fn is_unicode_punctuation(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch.is_ascii_punctuation() || ch.is_ascii_graphic() && !ch.is_alphanumeric())
}

// gfm[impl inlines.emphasis-and-strong-emphasis.multiple-of-three-condition-explanation]
// gfm[impl inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-alternative]
fn multiple_of_three_blocked(
    opener_count: usize,
    closer_count: usize,
    opener_can_close: bool,
    closer_can_open: bool,
) -> bool {
    let one_can_both = opener_can_close || closer_can_open;
    one_can_both
        && (opener_count + closer_count) % 3 == 0
        && !(opener_count % 3 == 0 && closer_count % 3 == 0)
}

fn normalize_code_span(code: &str) -> String {
    let trimmed = if code.starts_with(' ') && code.ends_with(' ') && code.len() > 1 {
        &code[1..code.len() - 1]
    } else {
        code
    };
    trimmed.to_owned()
}

fn is_valid_tilde_opener(input: &str, index: usize, delim_len: usize) -> bool {
    !char_after(input, index + delim_len).is_none_or(char::is_whitespace)
}

fn is_valid_tilde_closer(input: &str, index: usize) -> bool {
    !char_before(input, index).is_none_or(char::is_whitespace)
}

fn code_span_end_at(input: &str, index: usize) -> Option<usize> {
    let remaining = input.get(index..)?;
    if !remaining.starts_with('`') {
        return None;
    }

    let run = remaining.chars().take_while(|ch| *ch == '`').count();
    let closer = "`".repeat(run);
    let search_start = index + run;
    let rest = input.get(search_start..)?;
    let offset = rest.find(&closer)?;
    Some(search_start + offset + closer.len())
}

fn find_delimited_closer(input: &str, mut index: usize, delim: &str) -> Option<usize> {
    while index < input.len() {
        if let Some(end) = code_span_end_at(input, index) {
            index = end;
            continue;
        }

        if input[index..].starts_with(delim) {
            if delim != "~~" || is_valid_tilde_closer(input, index) {
                return Some(index);
            }
            index += delim.len();
            continue;
        }

        let ch = input[index..].chars().next()?;
        index += ch.len_utf8();
    }

    None
}

// gfm[impl inlines.links.link-label-definition]
// gfm[impl inlines.links.link-label-ends-at-first-unescaped-closing-bracket]
// gfm[impl inlines.links.link-label-cannot-contain-unescaped-brackets]
// gfm[impl inlines.links.link-label-length-is-at-most-999-characters]
// gfm[impl inlines.links.link-label-must-contain-at-least-one-non-whitespace-character]
// gfm[impl inlines.links.link-label-must-contain-non-whitespace]
// gfm[impl inlines.links.link-labels-cannot-contain-unescaped-brackets]
fn find_closing_bracket(input: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (offset, ch) in input[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    return Some(start + offset);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

// gfm[impl inlines.links.link-destination-definition]
// gfm[impl inlines.links.destination-allows-normal-backslash-escaping]
// gfm[impl inlines.links.destination-cannot-contain-line-breaks]
// gfm[impl inlines.links.destination-may-contain-right-parenthesis-in-angle-brackets]
// gfm[impl inlines.links.destination-may-contain-spaces-only-in-angle-brackets]
// gfm[impl inlines.links.destination-may-include-fragments-and-queries]
// gfm[impl inlines.links.destination-preserves-url-escaping-and-allows-entity-references]
// gfm[impl inlines.links.balanced-parentheses-are-allowed-in-destination]
// gfm[impl inlines.links.implementations-may-limit-parenthesis-nesting-in-destinations]
// gfm[impl inlines.links.parentheses-in-destination-may-be-escaped]
// gfm[impl inlines.links.unbalanced-parentheses-require-escaping-or-angle-brackets]
// gfm[impl inlines.links.unmatched-angle-brackets-do-not-form-destination]
// gfm[impl inlines.links.angle-brackets-around-destination-must-be-unescaped]
// gfm[impl inlines.links.backslash-before-non-escapable-character-is-literal-in-destination]
// gfm[impl inlines.links.trailing-bracket-is-not-escaped-by-backslash-before-backslash]
fn parse_inline_link_target(rest: &str) -> Option<(usize, String, Option<String>)> {
    let closing = rest.find(')')?;
    let body = rest[..closing].trim();
    if body.is_empty() {
        return None;
    }

    if let Some((destination, title)) = split_destination_and_title(body) {
        return Some((closing + 2, destination, title));
    }

    Some((closing + 2, body.to_owned(), None))
}

// gfm[impl inlines.links.link-title-definition]
// gfm[impl inlines.links.titles-may-use-single-quotes-double-quotes-or-parentheses]
// gfm[impl inlines.links.titles-allow-backslash-escapes-and-entity-references]
// gfm[impl inlines.links.link-titles-may-span-lines-but-not-blank-lines]
// gfm[impl inlines.links.nested-balanced-quotes-in-titles-require-escaping]
// gfm[impl inlines.links.different-title-delimiter-can-avoid-escaping]
// gfm[impl inlines.links.title-cannot-appear-without-destination]
// gfm[impl inlines.links.title-may-be-omitted]
// gfm[impl inlines.links.title-must-be-separated-from-destination-by-whitespace]
fn split_destination_and_title(body: &str) -> Option<(String, Option<String>)> {
    for marker in ['"', '\'', '('] {
        let Some(index) = body.find(marker) else {
            continue;
        };
        let destination = body[..index].trim();
        let title_part = body[index..].trim();
        if destination.is_empty() {
            continue;
        }

        let title = match marker {
            '"' if title_part.ends_with('"') && title_part.len() >= 2 => {
                Some(title_part[1..title_part.len() - 1].to_owned())
            }
            '\'' if title_part.ends_with('\'') && title_part.len() >= 2 => {
                Some(title_part[1..title_part.len() - 1].to_owned())
            }
            '(' if title_part.ends_with(')') && title_part.len() >= 2 => {
                Some(title_part[1..title_part.len() - 1].to_owned())
            }
            _ => None,
        };
        if let Some(title) = title {
            return Some((destination.to_owned(), Some(title)));
        }
    }
    None
}

fn classify_html_block_start(line: &str) -> Option<HtmlBlockStart> {
    let trimmed = trimmed_html_block_line(line)?;
    if starts_html_type_1(trimmed) {
        return Some(HtmlBlockStart {
            kind: HtmlBlockKind::Type1ScriptStylePreTextarea,
            can_interrupt_paragraph: true,
        });
    }
    if starts_html_type_2(trimmed) {
        return Some(HtmlBlockStart {
            kind: HtmlBlockKind::Type2Comment,
            can_interrupt_paragraph: true,
        });
    }
    if starts_html_type_3(trimmed) {
        return Some(HtmlBlockStart {
            kind: HtmlBlockKind::Type3ProcessingInstruction,
            can_interrupt_paragraph: true,
        });
    }
    if starts_html_type_4(trimmed) {
        return Some(HtmlBlockStart {
            kind: HtmlBlockKind::Type4Declaration,
            can_interrupt_paragraph: true,
        });
    }
    if starts_html_type_5(trimmed) {
        return Some(HtmlBlockStart {
            kind: HtmlBlockKind::Type5Cdata,
            can_interrupt_paragraph: true,
        });
    }
    if starts_html_type_6(trimmed) {
        return Some(HtmlBlockStart {
            kind: HtmlBlockKind::Type6BlockTag,
            can_interrupt_paragraph: true,
        });
    }
    if starts_html_type_7(trimmed) {
        return Some(HtmlBlockStart {
            kind: HtmlBlockKind::Type7GenericTag,
            can_interrupt_paragraph: false,
        });
    }
    None
}

fn html_block_is_closed_by_line(kind: HtmlBlockKind, line: &str) -> bool {
    match kind {
        HtmlBlockKind::Type1ScriptStylePreTextarea => {
            contains_html_closing_tag(line, HTML_BLOCK_TYPE_1_TAGS)
        }
        HtmlBlockKind::Type2Comment => line.contains("-->"),
        HtmlBlockKind::Type3ProcessingInstruction => line.contains("?>"),
        HtmlBlockKind::Type4Declaration => line.contains('>'),
        HtmlBlockKind::Type5Cdata => line.contains("]]>"),
        HtmlBlockKind::Type6BlockTag | HtmlBlockKind::Type7GenericTag => false,
    }
}

// gfm[impl leaf-blocks.html-blocks.opening-tag-indent-up-to-three-spaces]
fn trimmed_html_block_line(line: &str) -> Option<&str> {
    let indent = leading_indent(line);
    (indent.columns <= 3).then_some(&line[indent.bytes..])
}

fn starts_html_type_1(line: &str) -> bool {
    HTML_BLOCK_TYPE_1_TAGS
        .iter()
        .any(|tag| matches_html_tag_prefix(line, tag))
}

fn starts_html_type_2(line: &str) -> bool {
    line.starts_with("<!--")
}

fn starts_html_type_3(line: &str) -> bool {
    line.starts_with("<?")
}

fn starts_html_type_4(line: &str) -> bool {
    line.starts_with("<!")
        && !line.starts_with("<!--")
        && !line.starts_with("<![CDATA[")
        && line
            .chars()
            .nth(2)
            .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn starts_html_type_5(line: &str) -> bool {
    line.starts_with("<![CDATA[")
}

fn starts_html_type_6(line: &str) -> bool {
    let Some(name) = parse_html_block_tag_start_name(line) else {
        return false;
    };
    is_html_block_tag_name(name, HTML_BLOCK_TYPE_6_TAGS)
}

fn starts_html_type_7(line: &str) -> bool {
    looks_like_complete_html_tag_line(line)
}

fn parse_html_block_tag_start_name(line: &str) -> Option<&str> {
    let mut line = line.strip_prefix('<')?;
    if let Some(rest) = line.strip_prefix('/') {
        line = rest;
    }

    let (name, consumed) = parse_html_tag_name_prefix(line)?;
    let rest = &line[consumed..];
    if rest.is_empty()
        || rest.starts_with('>')
        || rest.starts_with("/>")
        || rest.chars().next().is_some_and(is_html_whitespace)
    {
        Some(name)
    } else {
        None
    }
}

fn is_html_block_tag_name(name: &str, set: &[&str]) -> bool {
    set.iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

fn matches_html_tag_prefix(line: &str, name: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let open = format!("<{name}");
    if lower.starts_with(&open) {
        return lower[open.len()..]
            .chars()
            .next()
            .is_none_or(|ch| matches!(ch, ' ' | '\t' | '>' | '/'));
    }
    false
}

fn looks_like_complete_html_tag_line(line: &str) -> bool {
    parse_complete_html_tag_line(line).is_some()
}

// gfm[impl inlines.raw-html.tag-name-definition]
fn parse_html_tag_name_prefix(line: &str) -> Option<(&str, usize)> {
    let mut chars = line.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }

    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }

    Some((&line[..end], end))
}

// gfm[impl inlines.raw-html.attribute-name-definition]
fn parse_html_attribute_name_prefix(line: &str) -> Option<usize> {
    let mut chars = line.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || matches!(first, '_' | ':')) {
        return None;
    }

    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-') {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }

    Some(end)
}

// gfm[impl inlines.raw-html.unquoted-attribute-value-definition]
// gfm[impl inlines.raw-html.single-quoted-attribute-value-definition]
// gfm[impl inlines.raw-html.double-quoted-attribute-value-definition]
fn parse_html_attribute_value_prefix(line: &str) -> Option<usize> {
    let quote = line.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = line[1..].find(quote)? + 2;
        return Some(end);
    }

    let mut end = 0;
    for (index, ch) in line.char_indices() {
        if is_html_whitespace(ch) || matches!(ch, '"' | '\'' | '=' | '<' | '>' | '`') {
            break;
        }
        end = index + ch.len_utf8();
    }

    (end > 0).then_some(end)
}

// gfm[impl inlines.raw-html.attribute-definition]
// gfm[impl inlines.raw-html.attribute-value-specification-definition]
// gfm[impl inlines.raw-html.open-tag-definition]
fn parse_html_open_tag_prefix(line: &str) -> Option<(&str, usize)> {
    let rest = line.strip_prefix('<')?;
    if rest.starts_with('/') {
        return None;
    }

    let (name, name_len) = parse_html_tag_name_prefix(rest)?;
    let mut consumed = 1 + name_len;

    loop {
        let rest = &line[consumed..];
        if rest.starts_with('>') {
            return Some((name, consumed + 1));
        }
        if rest.starts_with("/>") {
            return Some((name, consumed + 2));
        }

        let whitespace = count_html_whitespace(rest);
        if whitespace == 0 {
            return None;
        }
        consumed += whitespace;

        let rest = &line[consumed..];
        if rest.starts_with('>') {
            return Some((name, consumed + 1));
        }
        if rest.starts_with("/>") {
            return Some((name, consumed + 2));
        }

        consumed += parse_html_attribute_name_prefix(rest)?;
        consumed += count_html_whitespace(&line[consumed..]);

        if line[consumed..].starts_with('=') {
            consumed += 1;
            consumed += count_html_whitespace(&line[consumed..]);
            consumed += parse_html_attribute_value_prefix(&line[consumed..])?;
        }
    }
}

// gfm[impl inlines.raw-html.closing-tag-definition]
fn parse_html_closing_tag_prefix(line: &str) -> Option<(&str, usize)> {
    let line = line.strip_prefix("</")?;
    let (name, name_len) = parse_html_tag_name_prefix(line)?;
    let whitespace = count_html_whitespace(&line[name_len..]);
    let rest = &line[name_len + whitespace..];
    rest.starts_with('>')
        .then_some((name, 2 + name_len + whitespace + 1))
}

fn parse_complete_html_tag_line(line: &str) -> Option<&str> {
    if let Some((name, consumed)) = parse_html_open_tag_prefix(line) {
        return line[consumed..].trim().is_empty().then_some(name);
    }

    let (name, consumed) = parse_html_closing_tag_prefix(line)?;
    line[consumed..].trim().is_empty().then_some(name)
}

fn contains_html_closing_tag(line: &str, names: &[&str]) -> bool {
    let mut search = line;
    let mut offset = 0;

    while let Some(index) = search.find("</") {
        offset += index;
        if let Some((name, _)) = parse_html_closing_tag_prefix(&line[offset..])
            && is_html_block_tag_name(name, names)
        {
            return true;
        }
        offset += 2;
        search = &line[offset..];
    }

    false
}

fn count_html_whitespace(line: &str) -> usize {
    line.chars()
        .take_while(|ch| is_html_whitespace(*ch))
        .count()
}

fn is_html_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r')
}

fn looks_like_inline_tag(body: &str) -> bool {
    let body = body.trim();
    if body.is_empty() {
        return false;
    }

    let first = body.chars().next().unwrap_or(' ');
    first == '/' || first == '!' || first == '?' || first.is_ascii_alphabetic()
}

fn parse_inline_children<Ext, Meta>(
    text: &str,
    inline_extensions: &[Box<dyn InlineExtension<Ext, Meta>>],
    refs: &ReferenceDefinitions,
    meta: Meta,
    provisional: bool,
) -> Result<Vec<Node<Ext, Meta>>, Error>
where
    Meta: Default + Clone,
    Ext: Clone + PartialEq,
{
    let mut parser = InlineParser {
        input: text,
        index: 0,
        inline_extensions,
        refs,
        meta,
        provisional,
    };
    parser.parse_until(None)
}

// gfm[impl inlines.images.html-rendering-typically-uses-plain-description-as-alt]
fn build_link_or_image<Ext, Meta>(
    image: bool,
    destination: String,
    title: Option<String>,
    children: Vec<Node<Ext, Meta>>,
    meta: Meta,
) -> Node<Ext, Meta>
where
    Meta: Clone,
{
    if image {
        Node::Image {
            destination,
            title,
            alt: plain_text(&children),
            meta,
        }
    } else {
        Node::Link {
            destination,
            title,
            children,
            meta,
        }
    }
}

fn build_item_node<Ext, Meta>(children: Vec<ParsedBlock<Ext, Meta>>) -> Node<Ext, Meta>
where
    Meta: Default,
{
    let mut iter = children.into_iter();
    match iter.next() {
        Some(ParsedBlock {
            node:
                Node::TaskItem {
                    checked,
                    children,
                    meta,
                },
            ..
        }) if iter.next().is_none() => Node::TaskItem {
            checked,
            children,
            meta,
        },
        first => {
            let mut all = Vec::new();
            if let Some(first) = first {
                all.push(first.node);
            }
            all.extend(iter.map(|child| child.node));
            Node::Item {
                children: all,
                meta: Meta::default(),
            }
        }
    }
}

// gfm[impl container-blocks.task-list-items-extension.definition]
// gfm[impl container-blocks.task-list-items-extension.marker-structure]
// gfm[impl container-blocks.task-list-items-extension.whitespace-marker-is-unchecked]
fn normalize_task_item<Ext, Meta>(
    mut children: Vec<ParsedBlock<Ext, Meta>>,
) -> Vec<ParsedBlock<Ext, Meta>>
where
    Meta: Default,
{
    let Some(first) = children.first_mut() else {
        return children;
    };
    let Node::Paragraph {
        children: inlines, ..
    } = &mut first.node
    else {
        return children;
    };
    let Some(Node::Text { text, .. }) = inlines.first_mut() else {
        return children;
    };
    let Some((checked, rest)) = parse_task_marker(text) else {
        return children;
    };
    *text = rest.to_owned();
    let paragraph_children = core::mem::take(inlines);
    first.node = Node::TaskItem {
        checked,
        children: vec![Node::Paragraph {
            children: paragraph_children,
            meta: Meta::default(),
        }],
        meta: Meta::default(),
    };
    children
}

fn parse_task_marker(text: &str) -> Option<(bool, &str)> {
    if let Some(rest) = text.strip_prefix("[ ] ") {
        return Some((false, rest));
    }
    if let Some(rest) = text.strip_prefix("[x] ") {
        return Some((true, rest));
    }
    if let Some(rest) = text.strip_prefix("[X] ") {
        return Some((true, rest));
    }
    None
}

fn is_tight_list<Ext, Meta>(items: &[ParsedListItem<Ext, Meta>]) -> bool {
    !items.iter().any(|item| item.loose)
}

fn is_lazy_list_continuation(line: &str, lines: &[String], index: usize) -> bool {
    !line.trim().is_empty()
        && parse_list_marker(line).is_none()
        && strip_block_quote_marker(line).is_none()
        && strip_indented_code_line(line).is_none()
        && parse_fence_opener(line).is_none()
        && !is_thematic_break(line)
        && parse_reference_definition(lines, index).is_none()
        && !matches!(parse_table_delimiter(line), Some(_))
        && !classify_html_block_start(line).is_some_and(|start| start.can_interrupt_paragraph)
        && !matches!(trim_up_to_three_spaces(line).chars().next(), Some('#'))
        && !(line.contains('|')
            && index + 1 < lines.len()
            && parse_table_delimiter(&lines[index + 1]).is_some())
}

// gfm[impl leaf-blocks.tables-extension.delimiter-row-structure]
fn parse_table_delimiter(line: &str) -> Option<Vec<Option<TableAlignment>>> {
    let cells = split_table_row(line);
    if cells.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(cells.len());
    for cell in cells {
        let trimmed = cell.trim();
        if trimmed.len() < 3 || !trimmed.chars().all(|ch| matches!(ch, '-' | ':')) {
            return None;
        }
        let alignment = if trimmed.starts_with(':') && trimmed.ends_with(':') {
            Some(TableAlignment::Center)
        } else if trimmed.starts_with(':') {
            Some(TableAlignment::Left)
        } else if trimmed.ends_with(':') {
            Some(TableAlignment::Right)
        } else {
            None
        };
        if trimmed.trim_matches(':').chars().all(|ch| ch == '-') {
            out.push(alignment);
        } else {
            return None;
        }
    }
    Some(out)
}

fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let core = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let core = core.strip_suffix('|').unwrap_or(core);
    core.split('|').map(|cell| cell.trim().to_owned()).collect()
}

fn plain_text<Ext, Meta>(nodes: &[Node<Ext, Meta>]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text { text, .. } => out.push_str(text),
            Node::CodeSpan { code, .. } => out.push_str(code),
            Node::SoftBreak { .. } | Node::HardBreak { .. } => out.push(' '),
            Node::Emphasis { children, .. }
            | Node::Strong { children, .. }
            | Node::Strikethrough { children, .. }
            | Node::Link { children, .. }
            | Node::Paragraph { children, .. }
            | Node::Heading { children, .. }
            | Node::BlockQuote { children, .. }
            | Node::List { children, .. }
            | Node::Item { children, .. }
            | Node::TaskItem { children, .. }
            | Node::Document { children, .. }
            | Node::Custom { children, .. } => out.push_str(&plain_text(children)),
            Node::Table { header, rows, .. } => {
                out.push_str(&plain_text(header));
                out.push_str(&plain_text(rows));
            }
            Node::TableRow { cells, .. } => out.push_str(&plain_text(cells)),
            Node::TableCell { children, .. } => out.push_str(&plain_text(children)),
            Node::Image { alt, .. } => out.push_str(alt),
            Node::CodeBlock { literal, .. } => out.push_str(literal),
            Node::Html { html, .. } => out.push_str(html),
            Node::ThematicBreak { .. } => {}
        }
    }
    out
}

// gfm[impl inlines.links.label-match-definition]
// gfm[impl inlines.links.label-matching-collapses-consecutive-internal-whitespace]
// gfm[impl inlines.links.label-matching-uses-unicode-case-fold]
fn normalize_reference_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// gfm[impl inlines.entity-and-numeric-character-references.references-produce-unicode-characters]
// gfm[impl inlines.entity-and-numeric-character-references.decimal-numeric-reference-definition]
// gfm[impl inlines.entity-and-numeric-character-references.hex-reference-definition]
// gfm[impl inlines.entity-and-numeric-character-references.invalid-references-are-literal-text]
// gfm[impl inlines.entity-and-numeric-character-references.entity-references-definition]
// gfm[impl inlines.entity-and-numeric-character-references.named-entities-require-semicolon]
// gfm[impl inlines.entity-and-numeric-character-references.parsers-need-not-preserve-source-form]
// gfm[impl inlines.entity-and-numeric-character-references.references-are-literal-text-in-code]
// gfm[impl inlines.entity-and-numeric-character-references.references-are-not-recognized-in-code]
// gfm[impl inlines.entity-and-numeric-character-references.references-are-recognized-outside-code]
// gfm[impl inlines.entity-and-numeric-character-references.references-cannot-replace-structural-markers]
// gfm[impl inlines.entity-and-numeric-character-references.references-cannot-replace-structure-symbols]
// gfm[impl inlines.entity-and-numeric-character-references.unknown-named-entities-are-literal]
fn parse_entity_reference(input: &str) -> Option<(usize, String)> {
    if !input.starts_with('&') {
        return None;
    }
    let end = input.find(';')?;
    let entity = &input[1..end];
    let decoded = if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
    } else if let Some(num) = entity.strip_prefix('#') {
        num.parse::<u32>().ok().and_then(char::from_u32)
    } else {
        match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{00A0}'),
            _ => None,
        }
    }?;
    Some((end + 1, decoded.to_string()))
}

fn looks_like_url(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://")
}

fn looks_like_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn is_escapable(ch: char) -> bool {
    matches!(
        ch,
        '!' | '"'
            | '#'
            | '$'
            | '%'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | '+'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '<'
            | '='
            | '>'
            | '?'
            | '@'
            | '['
            | '\\'
            | ']'
            | '^'
            | '_'
            | '`'
            | '{'
            | '|'
            | '}'
            | '~'
    )
}

#[cfg(test)]
mod tests {
    use super::Parser;
    use crate::ext::{InlineExtension, InlineExtensionMatch};
    use crate::node::{Node, StreamItem};
    use crate::sink::VecSink;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestExt {
        Math(String),
    }

    struct MathExtension;

    impl InlineExtension<TestExt, ()> for MathExtension {
        fn try_parse(&self, input: &str) -> Option<InlineExtensionMatch<TestExt, ()>> {
            let rest = input.strip_prefix('$')?;
            let end = rest.find('$')?;
            Some(InlineExtensionMatch {
                consumed: end + 2,
                node: Node::Custom {
                    payload: TestExt::Math(rest[..end].to_owned()),
                    children: Vec::new(),
                    meta: (),
                },
            })
        }
    }

    #[test]
    fn emits_stable_ids_for_provisional_emphasis_upgrade() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("*hello", &mut sink).unwrap();
        parser.push_str("*", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let items = sink.items();
        assert_eq!(items.len(), 3);

        let first = match &items[0] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };
        let second = match &items[1] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };
        let third = match &items[2] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };

        assert!(first.provisional);
        assert!(second.provisional);
        assert!(!third.provisional);
        assert_eq!(first.id, second.id);
        assert_eq!(second.id, third.id);
        assert!(matches!(
            &third.node,
            Node::Paragraph { children, .. }
                if matches!(children.as_slice(), [Node::Emphasis { children, .. }] if matches!(children.as_slice(), [Node::Text { text, .. }] if text == "hello"))
        ));
    }

    // gfm[verify leaf-blocks.setext-headings.structure]
    #[test]
    fn upgrades_setext_heading() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("Title\n", &mut sink).unwrap();
        parser.push_str("---\n", &mut sink).unwrap();

        let items = sink.items();
        assert_eq!(items.len(), 2);

        let first = match &items[0] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };
        let second = match &items[1] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };

        assert_eq!(first.id, second.id);
        assert!(first.provisional);
        assert!(!second.provisional);
        assert!(matches!(&second.node, Node::Heading { level, .. } if *level == 2));
    }

    // gfm[verify leaf-blocks.fenced-code-blocks.content-continues-until-matching-closing-fence]
    #[test]
    fn supports_fenced_code_blocks() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("```rust\nfn main() {}", &mut sink).unwrap();
        parser.push_str("\n```\n", &mut sink).unwrap();

        let items = sink.items();
        assert_eq!(items.len(), 2);

        let first = match &items[0] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };
        let second = match &items[1] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };

        assert!(first.provisional);
        assert!(!second.provisional);
        assert!(matches!(
            &second.node,
            Node::CodeBlock { info, literal, .. }
                if info.as_deref() == Some("rust") && literal == "fn main() {}"
        ));
    }

    #[test]
    fn supports_block_quotes() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str("> # Foo\n> bar\n> baz\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::BlockQuote { children, .. }
                if matches!(children.as_slice(), [Node::Heading { level, .. }, Node::Paragraph { .. }] if *level == 1)
        ));
    }

    #[test]
    fn supports_simple_lists() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("- one\n- two\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::List { ordered: false, children, .. }
                if matches!(children.as_slice(), [Node::Item { .. }, Node::Item { .. }])
        ));
    }

    #[test]
    fn supports_indented_code_blocks() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("    foo\n    bar\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::CodeBlock { info: None, literal, .. } if literal == "foo\nbar"
        ));
    }

    #[test]
    fn supports_inline_extension_hooks() {
        let mut parser = Parser::builder()
            .with_inline_extension(MathExtension)
            .build();
        let mut sink = VecSink::new();

        parser.push_str("before $x^2$ after\n", &mut sink).unwrap();

        let emission = match &sink.items()[0] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };

        assert!(matches!(
            &emission.node,
            Node::Paragraph { children, .. }
                if matches!(children.as_slice(), [
                    Node::Text { text: before, .. },
                    Node::Custom { payload, .. },
                    Node::Text { text: after, .. }
                ] if before == "before " && after == " after" && payload == &TestExt::Math("x^2".to_owned()))
        ));
    }

    // gfm[verify inlines.links.inline-link-definition]
    // gfm[verify inlines.images.definition]
    #[test]
    fn supports_inline_links_and_images() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str("[link](/dest \"title\") and ![alt](/img.png)\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        let Node::Paragraph { children, .. } = final_node else {
            panic!("expected paragraph")
        };
        assert!(children.iter().any(|child| {
            matches!(
                child,
                Node::Link { destination, title, children, .. }
                    if destination == "/dest"
                        && title.as_deref() == Some("title")
                        && matches!(children.as_slice(), [Node::Text { text, .. }] if text == "link")
            )
        }));
        assert!(children.iter().any(|child| {
            matches!(
                child,
                Node::Image { destination, alt, .. }
                    if destination == "/img.png" && alt == "alt"
            )
        }));
    }

    // gfm[verify leaf-blocks.link-reference-definitions.may-appear-before-or-after-uses]
    // gfm[verify inlines.links.shortcut-reference-link-definition]
    #[test]
    fn resolves_reference_links_from_later_definitions() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("[foo]\n", &mut sink).unwrap();
        parser.push_str("\n[foo]: /url\n", &mut sink).unwrap();

        let items = sink.items();
        assert_eq!(items.len(), 2);

        let first = match &items[0] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };
        let second = match &items[1] {
            StreamItem::Upsert(emission) => emission,
            item => panic!("unexpected item: {item:?}"),
        };

        assert_eq!(first.id, second.id);
        assert!(matches!(
            &second.node,
            Node::Paragraph { children, .. }
                if matches!(children.as_slice(), [Node::Link { destination, children: link_children, .. }]
                    if destination == "/url"
                    && matches!(link_children.as_slice(), [Node::Text { text, .. }] if text == "foo"))
        ));
    }

    // gfm[verify inlines.raw-html.html-like-tag-text-is-parsed-as-raw-html]
    #[test]
    fn supports_inline_raw_html() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str("before <span>hi</span> after\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        let Node::Paragraph { children, .. } = final_node else {
            panic!("expected paragraph")
        };
        let rendered = children
            .iter()
            .map(|child| match child {
                Node::Text { text, .. } => text.clone(),
                Node::Html { html, .. } => html.clone(),
                other => format!("{other:?}"),
            })
            .collect::<String>();
        assert_eq!(rendered, "before <span>hi</span> after");
    }

    // gfm[verify leaf-blocks.html-blocks.definition]
    #[test]
    fn supports_html_blocks() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("<div>\nfoo\n</div>\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::Html { html, .. } if html == "<div>\nfoo\n</div>"
        ));
    }

    // gfm[verify leaf-blocks.html-blocks.opening-tag-indent-up-to-three-spaces]
    #[test]
    fn html_blocks_only_start_with_up_to_three_spaces_of_indent() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str("   <div>\n\n    <div>\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let finals: Vec<_> = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();

        assert!(matches!(
            finals.as_slice(),
            [Node::Html { html: first, .. }, Node::CodeBlock { literal, .. }]
                if first == "   <div>" && literal == "<div>"
        ));
    }

    // gfm[verify leaf-blocks.html-blocks.type-7-example-cannot-interrupt-paragraph]
    #[test]
    fn type_7_html_blocks_do_not_interrupt_paragraphs() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str("Foo\n<a href=\"bar\">\nbaz\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::Paragraph { children, .. }
                if matches!(children.as_slice(), [
                    Node::Text { text: first, .. },
                    Node::SoftBreak { .. },
                    Node::Html { html, .. },
                    Node::SoftBreak { .. },
                    Node::Text { text: last, .. },
                ] if first == "Foo" && html == "<a href=\"bar\">" && last == "baz")
        ));
    }

    // gfm[verify inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
    // gfm[verify inlines.autolinks.definition]
    // gfm[verify inlines.autolinks-extension.extended-url-autolink-definition]
    #[test]
    fn supports_strikethrough_and_autolinks() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str(
                "~~gone~~ https://example.com &amp; <user@example.com>\n",
                &mut sink,
            )
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .last()
            .unwrap();

        let Node::Paragraph { children, .. } = final_node else {
            panic!("expected paragraph")
        };
        assert!(
            children
                .iter()
                .any(|child| matches!(child, Node::Strikethrough { .. }))
        );
        assert!(children.iter().any(|child| matches!(child, Node::Link { destination, .. } if destination == "https://example.com")));
        assert!(children.iter().any(|child| matches!(child, Node::Link { destination, .. } if destination == "mailto:user@example.com")));
        assert!(
            children
                .iter()
                .any(|child| matches!(child, Node::Text { text, .. } if text.contains('&')))
        );
    }

    // gfm[verify container-blocks.task-list-items-extension.definition]
    // gfm[verify leaf-blocks.tables-extension.table-definition]
    #[test]
    fn supports_task_list_items_and_tables() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str(
                "- [x] done\n\n| a | b |\n| :-- | --: |\n| 1 | 2 |\n",
                &mut sink,
            )
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let finals: Vec<_> = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();

        assert!(finals.iter().any(|node| matches!(node, Node::List { children, .. } if matches!(children.as_slice(), [Node::TaskItem { checked: true, .. }]))));
        assert!(finals.iter().any(|node| matches!(node, Node::Table { .. })));
    }

    // gfm[verify container-blocks.list-items.lazy-continuation-example]
    // gfm[verify container-blocks.lists.loose-list-definition]
    #[test]
    fn detects_loose_lists_and_lazy_continuation() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str("- one\n  still one\n\n- two\n\n  two para\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .find(|node| matches!(node, Node::List { .. }))
            .unwrap();

        assert!(matches!(
            final_node,
            Node::List { tight: Some(false), children, .. }
                if matches!(children.as_slice(), [Node::Item { children: first, .. }, Node::Item { children: second, .. }]
                    if matches!(first.as_slice(), [Node::Paragraph { children, .. }] if matches!(children.as_slice(), [Node::Text { text, .. }, Node::SoftBreak { .. }, Node::Text { text: second_line, .. }] if text == "one" && second_line == "still one"))
                    && second.len() == 2)
        ));
    }

    // gfm[verify container-blocks.list-items.ordered-start-number-is-at-most-nine-digits]
    // gfm[verify container-blocks.list-items.ordered-start-number-may-have-leading-zeroes]
    #[test]
    fn enforces_ordered_list_marker_width_rules() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str(
                "123456789. ok\n\n1234567890. not ok\n\n003. also ok\n",
                &mut sink,
            )
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let finals: Vec<_> = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();

        assert!(finals.iter().any(|node| matches!(
            node,
            Node::List {
                ordered: true,
                start: Some(123456789),
                ..
            }
        )));
    }

    // gfm[verify container-blocks.list-items.interrupting-ordered-list-must-start-at-one]
    #[test]
    fn ordered_lists_only_interrupt_paragraphs_at_one() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser
            .push_str("para\n2. no interrupt\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .next()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::Paragraph { children, .. }
                if matches!(children.as_slice(), [Node::Text { text, .. }, Node::SoftBreak { .. }, Node::Text { text: second, .. }] if text == "para" && second == "2. no interrupt")
        ));
    }

    // gfm[verify container-blocks.list-items.empty-item-does-not-require-post-marker-spaces]
    #[test]
    fn supports_empty_list_items_without_post_marker_space() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("*\n* next\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .find(|node| matches!(node, Node::List { .. }))
            .unwrap();

        assert!(matches!(
            final_node,
            Node::List { children, .. }
                if matches!(children.as_slice(), [Node::Item { children: first, .. }, Node::Item { children: second, .. }]
                    if first.is_empty()
                    && matches!(second.as_slice(), [Node::Paragraph { children, .. }] if matches!(children.as_slice(), [Node::Text { text, .. }] if text == "next")))
        ));
    }

    // gfm[verify container-blocks.list-items.interrupting-first-item-cannot-begin-with-blank-line]
    // gfm[verify container-blocks.list-items.blank-line-start-makes-post-marker-spacing-irrelevant]
    #[test]
    fn blank_start_list_item_does_not_interrupt_paragraph_and_uses_normalized_indent() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("para\n+\n  foo\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let finals: Vec<_> = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();

        assert!(matches!(finals.first(), Some(Node::Paragraph { .. })));
    }

    // gfm[verify inlines.emphasis-and-strong-emphasis.both-multiples-of-three-allow-emphasis-match]
    #[test]
    fn preserves_chunk_invariant_for_nested_delimiters() {
        let input = "foo***bar***baz\n";

        let mut one = Parser::<(), ()>::new();
        let mut one_sink = VecSink::new();
        one.push_str(input, &mut one_sink).unwrap();
        one.finish(&mut one_sink).unwrap();

        let mut chunked = Parser::<(), ()>::new();
        let mut chunked_sink = VecSink::new();
        for chunk in ["foo*", "**bar", "***baz\n"] {
            chunked.push_str(chunk, &mut chunked_sink).unwrap();
        }
        chunked.finish(&mut chunked_sink).unwrap();

        let one_final: Vec<_> = one_sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();
        let chunked_final: Vec<_> = chunked_sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();

        assert_eq!(one_final, chunked_final);
    }

    // gfm[verify container-blocks.list-items.sublists-follow-general-indentation-rules]
    #[test]
    fn supports_nested_sublists() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("- one\n  - two\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .find(|node| matches!(node, Node::List { .. }))
            .unwrap();

        assert!(matches!(
            final_node,
            Node::List { children, .. }
                if matches!(children.as_slice(), [Node::Item { children, .. }]
                    if matches!(children.as_slice(), [Node::Paragraph { .. }, Node::List { children: nested, .. }]
                        if matches!(nested.as_slice(), [Node::Item { .. }])))
        ));
    }

    // gfm[verify container-blocks.list-items.indented-code-must-be-four-spaces-beyond-content-indent]
    #[test]
    fn supports_indented_code_inside_list_items() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("- foo\n\n      bar\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .find(|node| matches!(node, Node::List { .. }))
            .unwrap();

        assert!(matches!(
            final_node,
            Node::List { children, .. }
                if matches!(children.as_slice(), [Node::Item { children, .. }]
                    if matches!(children.as_slice(), [Node::Paragraph { .. }, Node::CodeBlock { literal, .. }] if literal == "bar"))
        ));
    }

    // gfm[verify preliminaries.tabs.tabs-not-expanded-in-lines]
    #[test]
    fn preserves_tabs_in_non_structural_text() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("a\tb\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let final_node = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .next()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::Paragraph { children, .. }
                if matches!(children.as_slice(), [Node::Text { text, .. }] if text == "a\tb")
        ));
    }

    #[test]
    fn deletes_blocks_that_turn_into_reference_definitions() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("[foo]", &mut sink).unwrap();
        parser.push_str(": /url\n", &mut sink).unwrap();

        let items = sink.items();
        assert_eq!(items.len(), 2);
        let first_id = match &items[0] {
            StreamItem::Upsert(emission) => emission.id,
            item => panic!("unexpected item: {item:?}"),
        };
        assert!(matches!(items[1], StreamItem::Delete { id } if id == first_id));
    }

    #[test]
    fn preserves_chunk_invariant_for_supported_syntax() {
        let input = "# hello\n\nparagraph with *em* and `code`\n\n---\n";

        let mut one = Parser::<(), ()>::new();
        let mut one_sink = VecSink::new();
        one.push_str(input, &mut one_sink).unwrap();
        one.finish(&mut one_sink).unwrap();

        let mut chunked = Parser::<(), ()>::new();
        let mut chunked_sink = VecSink::new();
        for chunk in [
            "# he",
            "llo\n\npara",
            "graph with *em* and `code`\n",
            "\n---\n",
        ] {
            chunked.push_str(chunk, &mut chunked_sink).unwrap();
        }
        chunked.finish(&mut chunked_sink).unwrap();

        let one_final: Vec<_> = one_sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();
        let chunked_final: Vec<_> = chunked_sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(emission) if !emission.provisional => {
                    Some(emission.node.clone())
                }
                _ => None,
            })
            .collect();

        assert_eq!(one_final, chunked_final);
    }

    #[test]
    fn rejects_new_input_after_finish() {
        let mut parser = Parser::<(), ()>::new();
        let mut sink = VecSink::new();

        parser.push_str("hello", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        assert!(parser.push_str(" world", &mut sink).is_err());
    }
}
