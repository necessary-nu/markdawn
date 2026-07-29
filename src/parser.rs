use crate::error::{Error, StreamError};
use crate::ext::{BlockMatch, Extension, InlineMatch};
use crate::node::{CorrelationId, Emission, Node, SourceSpan, StreamItem, TableAlignment};
use crate::sink::Sink;
use std::collections::{HashMap, HashSet};

#[path = "parser_footnote.rs"]
mod footnote;

#[path = "parser_table.rs"]
mod table;

use footnote::{
    assign_footnote_ref_numbers, collect_footnote_def_lines, collect_footnote_refs,
    collect_raw_footnote_definitions, find_footnote_label_end,
};
use table::{parse_table_delimiter, split_table_row_spans, trimmed_line_range};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedBlock<Ext> {
    pub(crate) node: Node<Ext>,
    pub(crate) provisional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveBlock<Ext> {
    pub id: CorrelationId,
    pub node: Node<Ext>,
    pub provisional: bool,
}

/// Diff a previous list of live blocks against a freshly-parsed block list,
/// reusing IDs by position. Returns the new live-block list and the events
/// (Upsert / Delete) the consumer should emit to keep its rendered tree in
/// sync. Allocates new IDs from `next_id` for any blocks beyond the previous
/// list's length.
///
/// Used by both `Parser::push_str` (streaming append) and `EditableDocument`
/// (editor edits) — same protocol, two producers.
// [spec:mw:req:core.stream.slot-identity]
// [spec:mw:req:core.stream.change-only-upsert]
// [spec:mw:req:core.stream.batch-ordering]
pub(crate) fn reconcile_blocks<Ext>(
    previous: Vec<LiveBlock<Ext>>,
    blocks: Vec<ParsedBlock<Ext>>,
    next_id: &mut CorrelationId,
) -> (Vec<LiveBlock<Ext>>, Vec<StreamItem<Node<Ext>>>)
where
    Ext: Clone + PartialEq,
{
    let mut next_live = Vec::with_capacity(blocks.len());
    let mut events = Vec::new();

    for (index, block) in blocks.into_iter().enumerate() {
        let id = previous.get(index).map(|live| live.id).unwrap_or_else(|| {
            let id = *next_id;
            *next_id += 1;
            id
        });

        let should_emit = match previous.get(index) {
            Some(live) => live.node != block.node || live.provisional != block.provisional,
            None => true,
        };

        if should_emit {
            events.push(StreamItem::Upsert(Emission {
                id,
                provisional: block.provisional,
                node: block.node.clone(),
            }));
        }

        next_live.push(LiveBlock {
            id,
            node: block.node,
            provisional: block.provisional,
        });
    }

    for stale in previous.iter().skip(next_live.len()) {
        events.push(StreamItem::Delete { id: stale.id });
    }

    (next_live, events)
}

struct ParsedListItem<Ext> {
    block: ParsedBlock<Ext>,
    loose: bool,
    trailing_blank: bool,
}

type ReferenceDefinitions = HashMap<String, ReferenceDefinition>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceDefinition {
    destination: String,
    title: Option<String>,
}

type FootnoteLabels = HashSet<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawFootnoteDef {
    lines: Vec<String>,
    /// Line range the definition was written on, in the line list it was
    /// collected from.
    line_range: core::ops::Range<usize>,
}

type RawFootnoteDefs = HashMap<String, RawFootnoteDef>;

pub struct ParserBuilder<Ext = ()> {
    extensions: Vec<Box<dyn Extension<Ext>>>,
    gfm: bool,
    math: bool,
    alerts: bool,
    highlights: bool,
    definition_lists: bool,
    max_input_bytes: Option<usize>,
}

impl<Ext> Default for ParserBuilder<Ext> {
    fn default() -> Self {
        Self {
            extensions: Vec::new(),
            gfm: false,
            math: false,
            alerts: false,
            highlights: false,
            definition_lists: false,
            max_input_bytes: None,
        }
    }
}

impl<Ext> ParserBuilder<Ext> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_extension(mut self, extension: impl Extension<Ext> + 'static) -> Self {
        self.extensions.push(Box::new(extension));
        self
    }

    /// Enable GitHub Flavored Markdown extensions: tables, strikethrough,
    /// bare-URL autolinks, raw-HTML tagfilter, task lists, and footnotes.
    pub fn with_gfm(mut self) -> Self {
        self.gfm = true;
        self
    }

    /// Enable math extension: `$…$`, `$$…$$`, `\(…\)`, `\[…\]`.
    pub fn with_math(mut self) -> Self {
        self.math = true;
        self
    }

    /// Enable GitHub-style alert admonitions: `> [!NOTE]`, `> [!WARNING]`, etc.
    pub fn with_alerts(mut self) -> Self {
        self.alerts = true;
        self
    }

    /// Enable `==highlight==` inline syntax.
    pub fn with_highlights(mut self) -> Self {
        self.highlights = true;
        self
    }

    /// Enable definition list block syntax (`Term` followed by `: Definition`).
    pub fn with_definition_lists(mut self) -> Self {
        self.definition_lists = true;
        self
    }

    /// Enable all built-in extensions: GFM, math, alerts, highlights, and definition lists.
    pub fn with_all(self) -> Self {
        self.with_gfm()
            .with_math()
            .with_alerts()
            .with_highlights()
            .with_definition_lists()
    }

    /// Reject input if cumulative bytes across `push_str`, `finish`,
    /// and `parse_full_at` exceed `max`. Default is unlimited (no cap).
    /// Useful when rendering untrusted markdown where you want a hard
    /// upper bound before the parser starts allocating.
    // [spec:adv:req:markdown.input.byte-cap]
    pub fn with_max_input_bytes(mut self, max: usize) -> Self {
        self.max_input_bytes = Some(max);
        self
    }

    pub fn build(self) -> Parser<Ext> {
        Parser {
            buffer: String::new(),
            finished: false,
            extensions: self.extensions,
            gfm: self.gfm,
            math: self.math,
            alerts: self.alerts,
            highlights: self.highlights,
            definition_lists: self.definition_lists,
            live_blocks: Vec::new(),
            next_id: 1,
            max_input_bytes: self.max_input_bytes,
            bytes_seen: 0,
        }
    }
}

pub struct Parser<Ext = ()> {
    buffer: String,
    finished: bool,
    extensions: Vec<Box<dyn Extension<Ext>>>,
    gfm: bool,
    math: bool,
    alerts: bool,
    highlights: bool,
    definition_lists: bool,
    live_blocks: Vec<LiveBlock<Ext>>,
    next_id: CorrelationId,
    max_input_bytes: Option<usize>,
    bytes_seen: usize,
}

impl<Ext> Default for Parser<Ext> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Ext> Parser<Ext> {
    pub fn new() -> Self {
        Self::builder().build()
    }

    pub fn builder() -> ParserBuilder<Ext> {
        ParserBuilder::new()
    }

    // [spec:adv:req:markdown.input.byte-cap]
    pub fn push_str<S>(&mut self, chunk: &str, sink: &mut S) -> Result<(), StreamError<S::Error>>
    where
        S: Sink<Node<Ext>>,
        Ext: Clone + PartialEq,
    {
        if self.finished {
            return Err(Error::AlreadyFinished.into());
        }

        if chunk.is_empty() {
            return Ok(());
        }

        if let Some(cap) = self.max_input_bytes {
            let new_total = self.bytes_seen.saturating_add(chunk.len());
            if new_total > cap {
                return Err(Error::InputTooLarge {
                    seen: new_total,
                    cap,
                }
                .into());
            }
            self.bytes_seen = new_total;
        }

        self.buffer.push_str(&normalize_chunk(chunk));
        let blocks = self.parse_blocks(false)?;
        self.emit_blocks(blocks, sink)
    }

    pub fn finish<S>(&mut self, sink: &mut S) -> Result<(), StreamError<S::Error>>
    where
        S: Sink<Node<Ext>>,
        Ext: Clone + PartialEq,
    {
        if self.finished {
            return Err(Error::AlreadyFinished.into());
        }

        self.finished = true;
        let blocks = self.parse_blocks(true)?;
        self.emit_blocks(blocks, sink)
    }

    /// Parse a complete source string in one shot, bypassing the streaming
    /// state machine. Returns the parsed top-level blocks with their
    /// `source` spans populated to byte ranges relative to `base_offset`
    /// (the byte offset of `source` within an outer document). Used by
    /// `EditableDocument` for both full and block-window reparse.
    // [spec:adv:req:markdown.input.byte-cap]
    // [spec:mw:req:core.span.exact-extent]
    pub(crate) fn parse_full_at(
        &mut self,
        source: &str,
        base_offset: usize,
    ) -> Result<Vec<ParsedBlock<Ext>>, Error>
    where
        Ext: Clone + PartialEq,
    {
        if let Some(cap) = self.max_input_bytes
            && source.len() > cap
        {
            return Err(Error::InputTooLarge {
                seen: source.len(),
                cap,
            });
        }

        self.buffer.clear();
        self.live_blocks.clear();
        self.finished = false;
        self.buffer.push_str(&normalize_chunk(source));
        self.finished = true;

        let split = split_lines(&self.buffer);
        let line_offsets = compute_line_offsets(&self.buffer);
        let spans = LineSpans {
            offsets: &line_offsets,
            base: base_offset,
        };

        let refs = self.collect_reference_definitions(&split.lines)?;
        let raw_footnotes = if self.gfm {
            collect_raw_footnote_definitions(&split.lines)
        } else {
            HashMap::new()
        };
        let footnote_labels: FootnoteLabels = raw_footnotes.keys().cloned().collect();

        let mut ranges = Vec::new();
        let mut blocks = self.parse_block_lines_with_lazy(
            &split.lines,
            true,
            split.terminated,
            &refs,
            &footnote_labels,
            &[],
            Some(&mut ranges),
        )?;

        // Assign byte spans to top-level blocks based on captured line ranges,
        // then offset descendant inline spans (currently relative to the
        // inline input) into the same source coordinate space. `base_offset`
        // shifts everything so spans are valid in the outer source.
        for (block, range) in blocks.iter_mut().zip(ranges.iter()) {
            let start = spans.start_of(range.start);
            *block.node.source_mut() = SourceSpan::new(start, spans.start_of(range.end));
            offset_descendant_spans(&mut block.node, start);
        }

        if self.gfm {
            self.finalize_footnotes(
                &mut blocks,
                &raw_footnotes,
                &refs,
                &footnote_labels,
                true,
                spans,
            )?;
        }

        Ok(blocks)
    }

    // [spec:mw:req:core.sink.error-aborts]
    fn emit_blocks<S>(
        &mut self,
        blocks: Vec<ParsedBlock<Ext>>,
        sink: &mut S,
    ) -> Result<(), StreamError<S::Error>>
    where
        S: Sink<Node<Ext>>,
        Ext: Clone + PartialEq,
    {
        let previous = core::mem::take(&mut self.live_blocks);
        let (next_live, events) = reconcile_blocks(previous, blocks, &mut self.next_id);
        for event in events {
            sink.emit(event).map_err(StreamError::Sink)?;
        }
        self.live_blocks = next_live;
        Ok(())
    }

    // [spec:gfm:thm:blocks-and-inlines.precedence.two-phase-parsing]
    // [spec:gfm:def:blocks-and-inlines.document-is-sequence-of-blocks]
    // [spec:gfm:def:appendix-a-parsing-strategy.overview.document-is-represented-as-tree-of-blocks]
    // [spec:gfm:sem:appendix-a-parsing-strategy.overview.parsing-has-two-phases]
    // [spec:gfm:def:appendix-a-parsing-strategy.reference-implementations-use-this-parsing-strategy]
    fn parse_blocks(&self, finalizing: bool) -> Result<Vec<ParsedBlock<Ext>>, Error>
    where
        Ext: Clone + PartialEq,
    {
        let split = split_lines(&self.buffer);
        let refs = self.collect_reference_definitions(&split.lines)?;
        let raw_footnotes = if self.gfm {
            collect_raw_footnote_definitions(&split.lines)
        } else {
            HashMap::new()
        };
        let footnote_labels: FootnoteLabels = raw_footnotes.keys().cloned().collect();
        let mut blocks = self.parse_block_lines(
            &split.lines,
            finalizing,
            split.terminated,
            &refs,
            &footnote_labels,
        )?;
        if self.gfm {
            let line_offsets = compute_line_offsets(&self.buffer);
            self.finalize_footnotes(
                &mut blocks,
                &raw_footnotes,
                &refs,
                &footnote_labels,
                finalizing,
                LineSpans {
                    offsets: &line_offsets,
                    base: 0,
                },
            )?;
        }
        Ok(blocks)
    }

    // [spec:mw:req:core.ext.block-zero-consumed]
    fn try_block_extension(&self, lines: &[String], start: usize) -> Option<(usize, Node<Ext>)> {
        for extension in &self.extensions {
            if let Some(BlockMatch {
                consumed_lines,
                node,
            }) = extension.try_block(lines, start)
            {
                if consumed_lines == 0 {
                    continue;
                }
                return Some((consumed_lines, node));
            }
        }
        None
    }

    // [spec:mw:req:core.node.footnote-numbering]
    // [spec:mw:sem:core.span.unpopulated-is-empty+1]
    fn finalize_footnotes(
        &self,
        blocks: &mut Vec<ParsedBlock<Ext>>,
        raw_footnotes: &RawFootnoteDefs,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
        finalizing: bool,
        spans: LineSpans<'_>,
    ) -> Result<(), Error>
    where
        Ext: Clone + PartialEq,
    {
        if raw_footnotes.is_empty() {
            return Ok(());
        }

        let mut ordered_labels: Vec<String> = Vec::new();
        let mut occurrences: HashMap<String, u32> = HashMap::new();
        for block in blocks.iter() {
            collect_footnote_refs(&block.node, &mut ordered_labels, &mut occurrences);
        }

        if ordered_labels.is_empty() {
            return Ok(());
        }

        let mut label_numbers: HashMap<String, u32> = HashMap::new();
        for (idx, label) in ordered_labels.iter().enumerate() {
            label_numbers.insert(label.clone(), (idx + 1) as u32);
        }

        let mut counters: HashMap<String, u32> = HashMap::new();
        for block in blocks.iter_mut() {
            assign_footnote_ref_numbers(&mut block.node, &label_numbers, &mut counters);
        }

        let mut section_children: Vec<Node<Ext>> = Vec::with_capacity(ordered_labels.len());
        for label in ordered_labels.iter() {
            let number = label_numbers[label];
            let backref_count = *occurrences.get(label).unwrap_or(&0);
            let raw = raw_footnotes
                .get(label)
                .expect("ordered label must have a definition");
            let children_blocks =
                self.parse_block_lines(&raw.lines, finalizing, true, refs, footnote_labels)?;
            let children: Vec<Node<Ext>> = children_blocks.into_iter().map(|b| b.node).collect();
            // A definition spans the lines it was written on, wherever in the
            // document that was; the section it is lifted into does not.
            let start = spans.start_of(raw.line_range.start);
            let mut definition = Node::FootnoteDefinition {
                label: label.clone(),
                number,
                backref_count,
                children,
                source: SourceSpan::new(start, spans.start_of(raw.line_range.end)),
            };
            offset_descendant_spans(&mut definition, start);
            section_children.push(definition);
        }

        let provisional = blocks.iter().any(|b| b.provisional);
        let eof = spans.eof();
        blocks.push(ParsedBlock {
            node: Node::FootnoteSection {
                children: section_children,
                source: SourceSpan::new(eof, eof),
            },
            provisional,
        });

        Ok(())
    }

    // [spec:gfm:sem:leaf-blocks.link-reference-definitions.may-appear-before-or-after-uses]
    // [spec:gfm:sem:leaf-blocks.link-reference-definitions.scope-is-the-entire-document]
    // [spec:gfm:sem:leaf-blocks.link-reference-definitions.first-matching-definition-takes-precedence]
    // [spec:gfm:syn:leaf-blocks.link-reference-definitions.multiple-definitions-may-appear-consecutively]
    // [spec:gfm:sem:leaf-blocks.link-reference-definitions.unused-definitions-have-no-visible-output]
    // [spec:gfm:sem:leaf-blocks.link-reference-definitions.use-may-precede-definition]
    fn collect_reference_definitions(&self, lines: &[String]) -> Result<ReferenceDefinitions, Error>
    where
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
        Ext: Clone + PartialEq,
    {
        let mut index = 0;
        // Track whether the previous non-blank line was plain paragraph text.
        // A reference definition cannot appear on a paragraph continuation line.
        let mut in_paragraph = false;
        while index < lines.len() {
            if lines[index].trim().is_empty() {
                in_paragraph = false;
                index += 1;
                continue;
            }

            if !in_paragraph {
                // Skip footnote definitions — handled by collect_footnote_definitions.
                if self.gfm
                    && let Some((next, _, _)) = collect_footnote_def_lines(lines, index)
                {
                    index = next;
                    in_paragraph = false;
                    continue;
                }
                if let Some((next, (label, def))) = parse_reference_definition(lines, index) {
                    refs.entry(label).or_insert(def);
                    index = next;
                    // After a ref def the next line starts fresh (not a paragraph continuation).
                    in_paragraph = false;
                    continue;
                }
            }

            if let Some((next, inner_lines)) = collect_block_quote_lines(lines, index) {
                self.collect_reference_definitions_in_lines(&inner_lines, refs)?;
                index = next;
                in_paragraph = false;
                continue;
            }

            if let Some((next, item_lines)) = collect_list_item_lines(lines, index) {
                for item in item_lines {
                    self.collect_reference_definitions_in_lines(&item, refs)?;
                }
                index = next;
                in_paragraph = false;
                continue;
            }

            // Skip extension blocks so their content isn't scanned for link
            // reference definitions.
            if let Some((consumed, _)) = self.try_block_extension(lines, index) {
                index += consumed;
                in_paragraph = false;
                continue;
            }

            if let Some((next, _)) = self.parse_html_block(lines, index, true, true) {
                index = next;
                in_paragraph = false;
                continue;
            }

            if self.math
                && let Some((next, _)) = parse_math_block::<Ext>(lines, index, true)
            {
                index = next;
                in_paragraph = false;
                continue;
            }

            if let Some((next, _)) = self.parse_fenced_code(lines, index, true) {
                index = next;
                in_paragraph = false;
                continue;
            }

            if let Some((next, _)) = self.parse_indented_code(lines, index, true, true) {
                index = next;
                in_paragraph = false;
                continue;
            }

            // ATX heading and thematic break also reset paragraph state
            let empty_fn_labels: FootnoteLabels = FootnoteLabels::new();
            if self
                .parse_atx_heading(&lines[index], refs, &empty_fn_labels)?
                .is_some()
                || is_thematic_break(&lines[index])
            {
                in_paragraph = false;
                index += 1;
                continue;
            }

            // This line is paragraph text; the next non-blank line is a
            // paragraph continuation and must not be treated as a ref def.
            in_paragraph = true;
            index += 1;
        }

        Ok(())
    }

    // [spec:gfm:def:leaf-blocks.leaf-blocks-make-up-markdown-document]
    // [spec:gfm:syn:blocks-and-inlines.precedence.block-indicators-take-precedence-over-inline-indicators]
    // [spec:gfm:def:blocks-and-inlines.container-and-leaf-blocks-definition]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.phase-1-line-processing-procedure]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.each-line-may-close-open-or-append]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.initial-tree-is-just-document]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.lines-can-be-discarded-after-incorporation]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.four-line-example-shows-tree-evolution]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.first-line-opens-block-quote-and-paragraph]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.second-line-is-lazy-continuation]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.third-line-closes-paragraph-and-opens-list]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.fourth-line-opens-second-list-item]
    fn parse_block_lines(
        &self,
        lines: &[String],
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<Vec<ParsedBlock<Ext>>, Error>
    where
        Ext: Clone + PartialEq,
    {
        self.parse_block_lines_with_lazy(
            lines,
            finalizing,
            terminated,
            refs,
            footnote_labels,
            &[],
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_block_lines_with_lazy(
        &self,
        lines: &[String],
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
        lazy: &[bool],
        mut ranges_out: Option<&mut Vec<core::ops::Range<usize>>>,
    ) -> Result<Vec<ParsedBlock<Ext>>, Error>
    where
        Ext: Clone + PartialEq,
    {
        let mut blocks = Vec::new();
        let mut index = 0;

        while index < lines.len() {
            let block_start = index;
            let blocks_len_before = blocks.len();

            'block: {
                // [spec:gfm:sem:leaf-blocks.blank-lines.blank-lines-definition]
                // [spec:gfm:sem:leaf-blocks.blank-lines.blank-lines-at-beginning-end-document-are-also-ignored]
                if lines[index].trim().is_empty() {
                    index += 1;
                    break 'block;
                }

                // Skip footnote definitions — they are collected separately and rendered
                // in a synthetic footnote section at the end of the document.
                if self.gfm
                    && let Some((next, _, _)) = collect_footnote_def_lines(lines, index)
                {
                    index = next;
                    break 'block;
                }

                if let Some((next, _)) = parse_reference_definition(lines, index) {
                    index = next;
                    break 'block;
                }

                // User-provided block extensions run before all built-in block
                // parsers so they can shadow any syntax.
                if let Some((consumed, node)) = self.try_block_extension(lines, index) {
                    blocks.push(ParsedBlock {
                        node,
                        provisional: false,
                    });
                    index += consumed;
                    break 'block;
                }

                if let Some((next, block)) =
                    self.parse_html_block(lines, index, finalizing, terminated)
                {
                    blocks.push(block);
                    index = next;
                    break 'block;
                }

                if self.gfm
                    && let Some((next, block)) = self.parse_table(
                        lines,
                        index,
                        finalizing,
                        terminated,
                        refs,
                        footnote_labels,
                    )?
                {
                    blocks.push(block);
                    index = next;
                    break 'block;
                }

                if let Some((next, block)) = self.parse_block_quote(
                    lines,
                    index,
                    finalizing,
                    terminated,
                    refs,
                    footnote_labels,
                ) {
                    blocks.push(block?);
                    index = next;
                    break 'block;
                }

                // [spec:gfm:syn:leaf-blocks.thematic-breaks.can-interrupt-paragraphs]
                // [spec:gfm:syn:leaf-blocks.thematic-breaks.need-not-be-surrounded-by-blank-lines]
                // [spec:gfm:syn:leaf-blocks.thematic-breaks.thematic-break-takes-precedence-over-list-item]
                // [spec:gfm:syn:leaf-blocks.thematic-breaks.use-different-bullet-for-break-inside-list-item]
                if is_thematic_break(&lines[index]) {
                    blocks.push(ParsedBlock {
                        node: Node::ThematicBreak {
                            source: SourceSpan::default(),
                        },
                        provisional: !finalizing && index + 1 == lines.len() && !terminated,
                    });
                    index += 1;
                    break 'block;
                }

                if let Some((next, block)) =
                    self.parse_list(lines, index, finalizing, terminated, refs, footnote_labels)?
                {
                    blocks.push(block);
                    index = next;
                    break 'block;
                }

                if self.math
                    && let Some((next, block)) = parse_math_block(lines, index, finalizing)
                {
                    blocks.push(block);
                    index = next;
                    break 'block;
                }

                if let Some((next, block)) = self.parse_fenced_code(lines, index, finalizing) {
                    blocks.push(block?);
                    index = next;
                    break 'block;
                }

                if let Some((next, block)) =
                    self.parse_indented_code(lines, index, finalizing, terminated)
                {
                    blocks.push(block);
                    index = next;
                    break 'block;
                }

                if let Some(block) = self.parse_atx_heading(&lines[index], refs, footnote_labels)? {
                    blocks.push(ParsedBlock {
                        node: block,
                        provisional: !finalizing && index + 1 == lines.len() && !terminated,
                    });
                    index += 1;
                    break 'block;
                }

                if self.definition_lists
                    && let Some((next, block)) =
                        self.parse_definition_list(lines, index, refs, footnote_labels)?
                {
                    blocks.push(block);
                    index = next;
                    break 'block;
                }

                let (next, block) = self.parse_paragraphish(
                    lines,
                    index,
                    finalizing,
                    terminated,
                    refs,
                    footnote_labels,
                    lazy,
                )?;
                blocks.push(block);
                index = next;
            }

            if blocks.len() > blocks_len_before
                && let Some(out) = ranges_out.as_deref_mut()
            {
                out.push(block_start..index);
            }
        }

        Ok(blocks)
    }

    // [spec:gfm:def:container-blocks.block-quotes.marker-structure]
    // [spec:gfm:syn:container-blocks.block-quotes.space-after-marker-is-optional]
    // [spec:gfm:syn:container-blocks.block-quotes.can-have-initial-or-final-blank-lines]
    // [spec:gfm:syn:container-blocks.block-quotes.basic-case]
    // [spec:gfm:syn:container-blocks.block-quotes.adjacent-lines-form-single-block-quote]
    // [spec:gfm:thm:container-blocks.block-quotes.blank-line-required-before-following-paragraph]
    // [spec:gfm:syn:container-blocks.block-quotes.blank-line-separates-adjacent-block-quotes]
    // [spec:gfm:syn:container-blocks.block-quotes.blank-lines-generally-not-required-around-block-quotes]
    // [spec:gfm:syn:container-blocks.block-quotes.blank-quoted-line-creates-two-paragraphs]
    // [spec:gfm:syn:container-blocks.block-quotes.can-be-empty]
    // [spec:gfm:syn:container-blocks.block-quotes.can-interrupt-paragraphs]
    // [spec:gfm:syn:container-blocks.block-quotes.consecutiveness]
    // [spec:gfm:sem:container-blocks.block-quotes.consecutiveness-rationale]
    // [spec:gfm:syn:container-blocks.block-quotes.example]
    // [spec:gfm:thm:container-blocks.block-quotes.indented-code-needs-five-spaces-after-marker]
    // [spec:gfm:syn:container-blocks.block-quotes.indented-dash-remains-paragraph-continuation-text]
    // [spec:gfm:syn:container-blocks.block-quotes.laziness]
    // [spec:gfm:syn:container-blocks.block-quotes.laziness-allows-omitted-continuation-markers]
    // [spec:gfm:syn:container-blocks.block-quotes.laziness-applies-only-to-paragraph-continuation-lines]
    // [spec:gfm:syn:container-blocks.block-quotes.laziness-does-not-apply-to-code-block-lines]
    // [spec:gfm:syn:container-blocks.block-quotes.lazy-continuation-example]
    // [spec:gfm:syn:container-blocks.block-quotes.lazy-continuation-list-example-explanation]
    // [spec:gfm:syn:container-blocks.block-quotes.list-example-block-quote-ends-after-first-line]
    // [spec:gfm:syn:container-blocks.block-quotes.marker-indent-four-spaces-starts-code-block]
    // [spec:gfm:syn:container-blocks.block-quotes.marker-indent-up-to-three-spaces]
    // [spec:gfm:syn:container-blocks.block-quotes.may-mix-lazy-and-non-lazy-continuation-lines]
    // [spec:gfm:thm:container-blocks.block-quotes.nested-block-quotes-may-omit-continuation-markers]
    // [spec:gfm:syn:container-blocks.block-quotes.omitting-marker-before-list-item-ends-block-quote]
    // [spec:gfm:syn:container-blocks.block-quotes.omitting-marker-before-setext-underline-changes-meaning]
    // [spec:gfm:syn:container-blocks.block-quotes.only-defined-cases-count]
    // [spec:gfm:def:container-blocks.block-quotes.paragraph-continuation-text-definition]
    fn parse_block_quote(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Option<(usize, Result<ParsedBlock<Ext>, Error>)>
    where
        Ext: Clone + PartialEq,
    {
        let mut inner_lines = Vec::new();
        let mut lazy_flags: Vec<bool> = Vec::new();
        let mut index = start;
        let mut saw_marker = false;

        while index < lines.len() {
            if let Some(stripped) = strip_block_quote_marker(&lines[index]) {
                inner_lines.push(stripped);
                lazy_flags.push(false);
                saw_marker = true;
                index += 1;
                continue;
            }

            if !saw_marker {
                break;
            }

            if lines[index].trim().is_empty() {
                break;
            }

            // Lazy continuation: the previous inner line must be non-blank paragraph text
            // (we don't lazy-continue after blank lines or after code blocks / other block constructs).
            if inner_lines
                .last()
                .is_some_and(|line| is_paragraph_continuation_candidate(line))
                && is_lazy_block_quote_continuation(&lines[index], lines, index, self.gfm)
            {
                inner_lines.push(lines[index].clone());
                lazy_flags.push(true);
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
            self.parse_block_lines_with_lazy(
                &inner_lines,
                finalizing,
                inner_terminated,
                refs,
                footnote_labels,
                &lazy_flags,
                None,
            )
            .map(|children| {
                let provisional = children.iter().any(|child| child.provisional);
                let mut children: Vec<Node<Ext>> =
                    children.into_iter().map(|child| child.node).collect();
                let node = if self.alerts {
                    try_promote_alert(&mut children).unwrap_or(Node::BlockQuote {
                        children,
                        source: SourceSpan::default(),
                    })
                } else {
                    Node::BlockQuote {
                        children,
                        source: SourceSpan::default(),
                    }
                };
                ParsedBlock { provisional, node }
            }),
        ))
    }

    fn parse_definition_list(
        &self,
        lines: &[String],
        start: usize,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<Option<(usize, ParsedBlock<Ext>)>, Error>
    where
        Ext: Clone + PartialEq,
    {
        if start + 1 >= lines.len() {
            return Ok(None);
        }
        if lines[start].trim().is_empty() || is_definition_line(&lines[start + 1]).is_none() {
            return Ok(None);
        }

        let mut children: Vec<Node<Ext>> = Vec::new();
        let mut index = start;

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                index += 1;
                if index >= lines.len() || is_definition_line(&lines[index]).is_none() {
                    break;
                }
                continue;
            }

            if is_definition_line(&lines[index]).is_some() {
                let mut def_lines = Vec::new();
                while index < lines.len() {
                    if let Some(content) = is_definition_line(&lines[index]) {
                        def_lines.push(content.to_string());
                        index += 1;
                    } else {
                        break;
                    }
                }
                let text = def_lines.join("\n");
                let inlines = self.parse_inline_nodes(&text, false, refs, footnote_labels)?;
                children.push(Node::DefinitionDescription {
                    children: vec![Node::Paragraph {
                        children: inlines,
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                });
            } else {
                if index > start
                    && (index + 1 >= lines.len() || is_definition_line(&lines[index + 1]).is_none())
                {
                    break;
                }
                let term_inlines =
                    self.parse_inline_nodes(lines[index].trim(), false, refs, footnote_labels)?;
                children.push(Node::DefinitionTerm {
                    children: term_inlines,
                    source: SourceSpan::default(),
                });
                index += 1;
            }
        }

        if children.is_empty() {
            return Ok(None);
        }

        Ok(Some((
            index,
            ParsedBlock {
                provisional: false,
                node: Node::DefinitionList {
                    children,
                    source: SourceSpan::default(),
                },
            },
        )))
    }

    // [spec:gfm:def:container-blocks.list-items.marker-definition]
    // [spec:gfm:sem:container-blocks.list-items.type-is-determined-by-marker]
    // [spec:gfm:def:container-blocks.lists.loose-list-definition]
    // [spec:gfm:def:container-blocks.lists.tight-list-definition]
    // [spec:gfm:def:container-blocks.lists.definition]
    // [spec:gfm:def:container-blocks.lists.ordered-and-bullet-list-definition]
    // [spec:gfm:def:container-blocks.lists.same-marker-type-definition]
    // [spec:gfm:def:container-blocks.lists.same-type-definition]
    // [spec:gfm:def:container-blocks.lists.start-number-definition]
    // [spec:gfm:sem:container-blocks.lists.loose-vs-tight-html-difference]
    // [spec:gfm:syn:container-blocks.lists.changing-marker-type-or-delimiter-starts-new-list]
    // [spec:gfm:syn:container-blocks.lists.any-number-of-blank-lines-may-separate-items]
    // [spec:gfm:syn:container-blocks.lists.blank-html-comment-separates-lists]
    // [spec:gfm:sem:container-blocks.lists.blank-line-before-empty-item-still-makes-list-loose]
    // [spec:gfm:sem:container-blocks.lists.blank-line-between-block-elements-in-item-makes-list-loose]
    // [spec:gfm:sem:container-blocks.lists.blank-line-between-item-blocks-makes-list-loose]
    // [spec:gfm:sem:container-blocks.lists.blank-line-between-items-makes-list-loose]
    // [spec:gfm:sem:container-blocks.lists.blank-line-inside-block-quote-does-not-make-list-loose]
    // [spec:gfm:sem:container-blocks.lists.blank-lines-inside-code-block-do-not-make-list-loose]
    // [spec:gfm:syn:container-blocks.lists.commonmark-allows-list-interruption]
    // [spec:gfm:sem:container-blocks.lists.consecutive-block-elements-without-blank-line-keep-list-tight]
    // [spec:gfm:sem:container-blocks.lists.corresponding-standalone-example-should-parse-the-same-way]
    // [spec:gfm:syn:container-blocks.lists.four-space-indented-marker-is-code-block]
    // [spec:gfm:syn:container-blocks.lists.items-may-be-separated-by-blank-lines]
    // [spec:gfm:syn:container-blocks.lists.items-need-not-use-uniform-indentation]
    // [spec:gfm:syn:container-blocks.lists.list-can-interrupt-paragraph-without-blank-line]
    // [spec:gfm:sem:container-blocks.lists.loose-sublist-does-not-necessarily-make-outer-list-loose]
    // [spec:gfm:sem:container-blocks.lists.markdown-pl-disallows-this-to-avoid-hard-wrapped-numeral-lists]
    // [spec:gfm:sem:container-blocks.lists.markdown-pl-still-allows-block-quotes-to-interrupt-paragraphs]
    // [spec:gfm:syn:container-blocks.lists.more-than-three-spaces-do-not-start-item]
    // [spec:gfm:syn:container-blocks.lists.nested-example-is-list-item-with-sublist]
    // [spec:gfm:syn:container-blocks.lists.only-lists-starting-with-one-may-interrupt-paragraphs]
    // [spec:gfm:sem:container-blocks.lists.outer-list-may-be-loose-while-inner-list-is-tight]
    // [spec:gfm:def:container-blocks.lists.principle-of-uniformity-definition]
    // [spec:gfm:sem:container-blocks.lists.single-paragraph-list-is-tight]
    // [spec:gfm:syn:container-blocks.lists.starting-with-one-can-still-cause-unintended-list-capture]
    // [spec:gfm:sem:container-blocks.lists.starting-with-one-should-prevent-most-spurious-captures]
    // [spec:gfm:thm:container-blocks.lists.uniformity-preserves-nested-list-meaning]
    // [spec:gfm:thm:container-blocks.lists.uniformity-supports-list-interruption]
    fn parse_list(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<Option<(usize, ParsedBlock<Ext>)>, Error>
    where
        Ext: Clone + PartialEq,
    {
        let Some(first_marker) = parse_list_marker(&lines[start]) else {
            return Ok(None);
        };

        let mut index = start;
        let mut items: Vec<ParsedListItem<Ext>> = Vec::new();
        while index < lines.len() {
            // A thematic break on a subsequent item line ends the list so the
            // break can be parsed as a top-level block (e.g. `* * *` inside a
            // bullet list started with `*`).
            if !items.is_empty() && is_thematic_break(&lines[index]) {
                break;
            }

            let Some(marker) = parse_list_marker(&lines[index]) else {
                break;
            };

            // Same list type: ordered must match, and marker char must match
            if marker.ordered != first_marker.ordered {
                break;
            }
            if marker.marker_char != first_marker.marker_char {
                break;
            }

            let (next, item) = self.parse_list_item(
                lines,
                index,
                marker,
                finalizing,
                terminated,
                refs,
                footnote_labels,
            )?;
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
                    source: SourceSpan::default(),
                },
            },
        )))
    }

    // [spec:gfm:syn:container-blocks.list-items.required-indentation-is-marker-width-plus-padding]
    // [spec:gfm:thm:container-blocks.list-items.sublists-follow-general-indentation-rules]
    // [spec:gfm:syn:container-blocks.list-items.lazy-continuation-example]
    // [spec:gfm:syn:container-blocks.list-items.item-blocks-may-be-separated-by-multiple-blank-lines]
    // [spec:gfm:syn:container-blocks.list-items.basic-case]
    // [spec:gfm:syn:container-blocks.list-items.basic-case-example-ordered-item]
    // [spec:gfm:syn:container-blocks.list-items.basic-case-example-source-blocks]
    // [spec:gfm:syn:container-blocks.list-items.basic-rules-apply-only-to-nonblank-or-code-start]
    // [spec:gfm:syn:container-blocks.list-items.blank-line-start-makes-post-marker-spacing-irrelevant]
    // [spec:gfm:syn:container-blocks.list-items.empty-bullet-item]
    // [spec:gfm:syn:container-blocks.list-items.empty-item-cannot-interrupt-paragraph]
    // [spec:gfm:syn:container-blocks.list-items.empty-lines-inside-list-item-need-not-be-indented]
    // [spec:gfm:syn:container-blocks.list-items.empty-ordered-item]
    // [spec:gfm:syn:container-blocks.list-items.examples-of-required-indentation]
    // [spec:gfm:sem:container-blocks.list-items.extra-indent-counts-as-code-content]
    // [spec:gfm:syn:container-blocks.list-items.far-right-text-may-still-be-outside-item]
    // [spec:gfm:syn:container-blocks.list-items.indented-code-example-with-wider-marker]
    // [spec:gfm:sem:container-blocks.list-items.indented-code-inside-item-preserves-empty-lines]
    // [spec:gfm:syn:container-blocks.list-items.indented-code-must-be-four-spaces-beyond-content-indent]
    // [spec:gfm:syn:container-blocks.list-items.indented-code-start-uses-one-space-after-marker]
    // [spec:gfm:syn:container-blocks.list-items.indenting-whole-item-four-spaces-produces-code-block]
    // [spec:gfm:syn:container-blocks.list-items.interrupting-first-item-cannot-begin-with-blank-line]
    // [spec:gfm:syn:container-blocks.list-items.interrupting-ordered-list-must-start-at-one]
    // [spec:gfm:syn:container-blocks.list-items.item-may-contain-heading]
    // [spec:gfm:syn:container-blocks.list-items.item-starting-with-blank-line]
    // [spec:gfm:syn:container-blocks.list-items.item-starting-with-indented-code]
    // [spec:gfm:syn:container-blocks.list-items.items-may-start-with-one-blank-line-and-still-be-nonempty]
    // [spec:gfm:syn:container-blocks.list-items.laziness-works-inside-nested-structures]
    // [spec:gfm:syn:container-blocks.list-items.lazy-continuation-may-delete-only-part-of-indent]
    // [spec:gfm:sem:container-blocks.list-items.leading-indent-up-to-three-spaces-can-be-normalized]
    // [spec:gfm:syn:container-blocks.list-items.list-may-be-first-block-in-item]
    // [spec:gfm:syn:container-blocks.list-items.list-may-start-or-end-with-empty-item]
    // [spec:gfm:syn:container-blocks.list-items.may-begin-with-at-most-one-blank-line]
    // [spec:gfm:syn:container-blocks.list-items.may-contain-any-kind-of-block]
    // [spec:gfm:sem:container-blocks.list-items.ordered-item-start-number-comes-from-marker]
    // [spec:gfm:syn:container-blocks.list-items.ordered-start-number-cannot-be-negative]
    // [spec:gfm:syn:container-blocks.list-items.required-indentation-is-relative-not-column-based]
    // [spec:gfm:syn:container-blocks.list-items.same-column-can-still-be-inside-item-in-nested-context]
    // [spec:gfm:syn:container-blocks.list-items.sublist-example-indent-increases-with-wider-marker]
    // [spec:gfm:syn:container-blocks.list-items.sublist-example-one-space-is-not-enough]
    // [spec:gfm:syn:container-blocks.list-items.sublist-example-three-spaces-not-enough-for-wider-marker]
    // [spec:gfm:syn:container-blocks.list-items.sublist-example-two-space-indent]
    // [spec:gfm:syn:container-blocks.list-items.thematic-break-line-is-not-a-list-item]
    // [spec:gfm:syn:container-blocks.list-items.motivation.fixed-indent-from-marker-is-an-alternative]
    // [spec:gfm:sem:container-blocks.list-items.motivation.fixed-margin-indentation-is-unintuitive]
    // [spec:gfm:syn:container-blocks.list-items.motivation.fixed-marker-indent-breaks-code-patterns]
    // [spec:gfm:sem:container-blocks.list-items.motivation.four-space-rule-is-arbitrary]
    // [spec:gfm:syn:container-blocks.list-items.motivation.four-space-rule-splits-list]
    // [spec:gfm:syn:container-blocks.list-items.motivation.indented-code-start-needs-special-treatment]
    // [spec:gfm:sem:container-blocks.list-items.motivation.markdown-pl-diverged-from-four-space-rule]
    // [spec:gfm:syn:container-blocks.list-items.motivation.marker-width-determines-indentation]
    // [spec:gfm:sem:container-blocks.list-items.motivation.no-rule-avoids-breaking-all-existing-documents]
    // [spec:gfm:syn:container-blocks.list-items.motivation.original-markdown-spec-quoted]
    // [spec:gfm:def:container-blocks.list-items.motivation.original-rule-implies-four-space-rule]
    // [spec:gfm:syn:container-blocks.list-items.motivation.preferred-parse-is-single-list]
    // [spec:gfm:syn:container-blocks.list-items.motivation.spec-preserves-eight-space-code-pattern]
    // [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-allows-underindented-content]
    // [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-block-quote-html]
    // [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-example-inside-block-quote]
    // [spec:gfm:syn:container-blocks.list-items.motivation.two-space-rule-example-single-list-item]
    // [spec:gfm:sem:container-blocks.list-items.motivation.two-space-rule-is-extremely-unintuitive]
    #[allow(clippy::too_many_arguments)]
    fn parse_list_item(
        &self,
        lines: &[String],
        start: usize,
        marker: ListMarker,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<(usize, ParsedListItem<Ext>), Error>
    where
        Ext: Clone + PartialEq,
    {
        let mut inner_lines = Vec::new();
        let starts_empty = marker.content.trim().is_empty();
        let mut in_fence: Option<(char, usize)> =
            parse_fence_opener(&marker.content).map(|f| (f.marker, f.count));
        // Track whether we're currently "inside" a sublist nested within this
        // item. Once a top-level list marker line opens a sublist, any
        // subsequent indented content (and blank lines surrounded by such
        // content) belongs to that sublist, not the outer item.
        let mut in_sublist = in_fence.is_none() && parse_list_marker(&marker.content).is_some();
        inner_lines.push(marker.content);
        let mut index = start + 1;
        let mut saw_blank_separator = false;
        let mut loose = false;

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                // An item that started with a blank line cannot absorb later
                // content. If the next non-blank line is another list marker,
                // we advance past the blank to let the outer loop consume it;
                // otherwise the item ends here.
                if starts_empty {
                    let mut peek = index;
                    while peek < lines.len() && lines[peek].trim().is_empty() {
                        peek += 1;
                    }
                    let next_is_marker = peek < lines.len()
                        && parse_list_marker(&lines[peek])
                            .is_some_and(|m| m.ordered == marker.ordered);
                    if next_is_marker {
                        index = peek;
                        loose = true;
                    }
                    break;
                }
                // Count blanks outside fences. The loose check below filters
                // out blanks that don't actually separate top-level blocks.
                if in_fence.is_none() {
                    saw_blank_separator = true;
                }
                inner_lines.push(String::new());
                index += 1;
                continue;
            }

            if let Some(stripped) = strip_list_continuation(&lines[index], marker.content_indent) {
                let stripped_indent = leading_indent(&stripped).columns;
                // Update fence state based on the stripped content
                let was_in_fence = in_fence.is_some();
                if let Some((m, c)) = in_fence {
                    if is_fence_closer(&stripped, m, c) {
                        in_fence = None;
                    }
                } else if let Some(f) = parse_fence_opener(&stripped) {
                    in_fence = Some((f.marker, f.count));
                }
                // Determine looseness BEFORE updating in_sublist: a new
                // top-level construct (paragraph, list, code block) following
                // a top-level blank makes the item loose. We use the OLD
                // in_sublist state — the new line opens a top-level block if
                // it's at indent 0 OR continues a code block (indent>0 only
                // counts as nested when we were already in a sublist).
                let was_in_sublist = in_sublist;
                let new_top_level_block = !was_in_fence
                    && (stripped_indent == 0 || (stripped_indent > 0 && !was_in_sublist));
                if saw_blank_separator && new_top_level_block {
                    loose = true;
                }
                // Sublist tracking: only relevant when not in a code fence.
                if in_fence.is_none() && stripped_indent == 0 {
                    in_sublist = parse_list_marker(&stripped).is_some();
                }
                inner_lines.push(stripped);
                saw_blank_separator = false;
                index += 1;
                continue;
            }

            // A thematic break on a non-continuation line ends the list item
            // (and the list) so it can be parsed as a top-level block.
            if is_thematic_break(&lines[index]) {
                break;
            }

            // Standard lazy continuation (line wouldn't start a new block)
            if !inner_lines
                .last()
                .is_some_and(|line| line.trim().is_empty())
                && is_lazy_list_continuation(&lines[index], lines, index, self.gfm)
            {
                if saw_blank_separator {
                    loose = true;
                }
                inner_lines.push(lines[index].clone());
                saw_blank_separator = false;
                index += 1;
                continue;
            }

            // Indented-code-looking line as lazy continuation: only if the
            // previous inner line is open paragraph text (since indented code
            // cannot interrupt an open paragraph).
            if !saw_blank_separator
                && inner_lines
                    .last()
                    .is_some_and(|line| is_paragraph_continuation_candidate(line))
                && strip_indented_code_line(&lines[index]).is_some()
                && is_lazy_block_quote_continuation(&lines[index], lines, index, self.gfm)
            {
                inner_lines.push(lines[index].clone());
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
        let children = self.parse_block_lines(
            &inner_lines,
            finalizing,
            inner_terminated,
            refs,
            footnote_labels,
        )?;
        let children = if self.gfm {
            normalize_task_item(children)
        } else {
            children
        };
        Ok((
            index,
            ParsedListItem {
                loose,
                trailing_blank: saw_blank_separator,
                block: ParsedBlock {
                    provisional: children.iter().any(|child| child.provisional),
                    node: build_item_node(children),
                },
            },
        ))
    }

    // [spec:gfm:def:leaf-blocks.indented-code-blocks.definition]
    // [spec:gfm:syn:leaf-blocks.indented-code-blocks.less-than-four-spaces-ends-block]
    // [spec:gfm:sem:leaf-blocks.indented-code-blocks.contents-are-literal-text]
    // [spec:gfm:syn:leaf-blocks.indented-code-blocks.blank-lines-separate-chunks]
    // [spec:gfm:sem:leaf-blocks.indented-code-blocks.surrounding-blank-lines-are-excluded]
    // [spec:gfm:sem:leaf-blocks.indented-code-blocks.trailing-spaces-are-preserved]
    // [spec:gfm:syn:leaf-blocks.indented-code-blocks.first-line-can-indented-more-than-four-spaces]
    // [spec:gfm:sem:leaf-blocks.indented-code-blocks.indent-beyond-four-is-preserved]
    // [spec:gfm:syn:leaf-blocks.indented-code-blocks.cannot-interrupt-paragraphs]
    // [spec:gfm:syn:leaf-blocks.indented-code-blocks.indented-code-block-cannot-interrupt-paragraph]
    // [spec:gfm:syn:leaf-blocks.indented-code-blocks.may-occur-adjacent-to-other-blocks]
    // [spec:gfm:syn:leaf-blocks.indented-code-blocks.list-item-interpretation-takes-precedence]
    fn parse_indented_code(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
    ) -> Option<(usize, ParsedBlock<Ext>)>
    where
        Ext: Clone + PartialEq,
    {
        let first = strip_indented_code_line(&lines[start])?;
        let mut literal = first;
        let mut index = start + 1;
        let mut pending_blanks: Vec<String> = Vec::new();

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                // Preserve whitespace content beyond the 4-space indent
                let blank_content = strip_indent_columns(&lines[index], 4).unwrap_or_default();
                pending_blanks.push(blank_content);
                index += 1;
                continue;
            }

            let Some(stripped) = strip_indented_code_line(&lines[index]) else {
                break;
            };
            for blank in &pending_blanks {
                literal.push('\n');
                literal.push_str(blank);
            }
            pending_blanks.clear();
            literal.push('\n');
            literal.push_str(&stripped);
            index += 1;
        }

        // Trailing blank lines don't belong to the code block
        index -= pending_blanks.len();
        literal.push('\n');
        Some((
            index,
            ParsedBlock {
                node: Node::CodeBlock {
                    info: None,
                    literal,
                    source: SourceSpan::default(),
                },
                provisional: index == lines.len() && !finalizing && !terminated,
            },
        ))
    }

    // [spec:gfm:def:leaf-blocks.html-blocks.definition]
    // [spec:gfm:syn:leaf-blocks.html-blocks.ends-at-matching-end-condition-or-container-end]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-1-script-pre-style-start-and-end-conditions]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-2-comment-start-and-end-conditions]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-3-processing-instruction-start-and-end-conditions]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-4-declaration-start-and-end-conditions]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-5-cdata-start-and-end-conditions]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-6-block-tag-start-and-blank-line-end]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-7-complete-tag-start-and-blank-line-end]
    // [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-5-end-at-matching-end-marker]
    // [spec:gfm:thm:leaf-blocks.html-blocks.types-1-through-5-may-contain-blank-lines]
    // [spec:gfm:syn:leaf-blocks.html-blocks.starts-after-up-to-three-spaces-of-indentation]
    // [spec:gfm:def:leaf-blocks.html-blocks.has-seven-types]
    // [spec:gfm:syn:leaf-blocks.html-blocks.end-marker-may-occur-on-same-line-as-start]
    // [spec:gfm:syn:leaf-blocks.html-blocks.single-line-block-when-start-and-end-both-match]
    // [spec:gfm:sem:leaf-blocks.html-blocks.trailing-text-after-end-marker-stays-inside-block]
    // [spec:gfm:syn:leaf-blocks.html-blocks.inner-html-does-not-change-active-html-block-type]
    // [spec:gfm:syn:leaf-blocks.html-blocks.inner-start-conditions-do-not-affect-parser-state]
    // [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-5-without-end-marker-close-at-container-end]
    // [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-6-need-not-be-preceded-by-blank-line]
    // [spec:gfm:syn:leaf-blocks.html-blocks.blank-line-ends-type-6-block-and-normal-parsing-resumes]
    // [spec:gfm:syn:leaf-blocks.html-blocks.blank-line-after-opening-tag-allows-following-markdown]
    // [spec:gfm:syn:leaf-blocks.html-blocks.blank-line-separates-two-html-blocks-with-markdown-between]
    // [spec:gfm:sem:leaf-blocks.html-blocks.blank-lines-can-usually-be-removed-except-in-pre-blocks]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-6-and-7-usually-require-following-blank-line]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-6-continues-through-apparent-markdown-until-blank-line]
    // [spec:gfm:sem:leaf-blocks.html-blocks.pasting-html-with-blank-lines-usually-still-works]
    // [spec:gfm:sem:leaf-blocks.html-blocks.differs-from-original-markdown-rule]
    // [spec:gfm:sem:leaf-blocks.html-blocks.original-markdown-rule-quote]
    // [spec:gfm:sem:leaf-blocks.html-blocks.original-markdown-rule-is-more-liberal-about-blank-lines]
    // [spec:gfm:sem:leaf-blocks.html-blocks.original-markdown-rule-is-more-restrictive-in-some-ways]
    // [spec:gfm:sem:leaf-blocks.html-blocks.most-implementations-do-not-follow-all-original-restrictions]
    // [spec:gfm:sem:leaf-blocks.html-blocks.markdown-1-attribute-convention-is-more-complex]
    // [spec:gfm:syn:leaf-blocks.html-blocks.block-vs-inline-tag-behavior]
    // [spec:gfm:syn:leaf-blocks.html-blocks.inline-html-when-tag-is-not-alone-on-line]
    // [spec:gfm:syn:leaf-blocks.html-blocks.indented-inner-tags-with-blank-lines-can-become-code-blocks]
    fn parse_html_block(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
    ) -> Option<(usize, ParsedBlock<Ext>)>
    where
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
                        source: SourceSpan::default(),
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
                    source: SourceSpan::default(),
                },
                provisional: !closed && !finalizing,
            },
        ))
    }

    // [spec:gfm:def:leaf-blocks.tables-extension.table-definition]
    // [spec:gfm:def:leaf-blocks.tables-extension.delimiter-row-structure]
    // [spec:gfm:syn:leaf-blocks.tables-extension.header-and-delimiter-row-must-have-matching-cell-count]
    // [spec:gfm:syn:leaf-blocks.tables-extension.table-ends-at-blank-line-or-other-block]
    // [spec:gfm:def:leaf-blocks.tables-extension.gfm-enables-table-extension]
    // [spec:gfm:syn:leaf-blocks.tables-extension.rows-consist-of-pipe-separated-cells]
    // [spec:gfm:sem:leaf-blocks.tables-extension.body-rows-may-have-fewer-or-more-cells-than-header]
    // [spec:gfm:syn:leaf-blocks.tables-extension.column-widths-and-edge-pipes-need-not-be-consistent]
    // [spec:gfm:sem:leaf-blocks.tables-extension.no-tbody-generated-when-there-are-no-body-rows]
    // [spec:gfm:syn:leaf-blocks.tables-extension.block-level-elements-cannot-appear-in-tables]
    // [spec:gfm:req:leaf-blocks.tables-extension.leading-and-trailing-pipes-are-recommended-for-clarity]
    fn parse_table(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<Option<(usize, ParsedBlock<Ext>)>, Error>
    where
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
        let header_cells = split_table_row_spans(&lines[start]);
        // A header row that disagrees with the delimiter row's column
        // count is not a table: reject the whole construct rather than
        // reshape the header.
        // [spec:mw:req:core.node.table-cell-alignment]
        if header_cells.is_empty() || header_cells.len() != alignments.len() {
            return Ok(None);
        }

        // Row and cell spans are byte offsets relative to the table's first
        // line, which is where the enclosing block's span starts; the
        // descendant-offset pass lifts them into source coordinates.
        // [spec:mw:sem:core.span.unpopulated-is-empty+1]
        let mut row_offset = lines[start].len() + 1 + lines[start + 1].len() + 1;

        let mut rows = Vec::new();
        let mut index = start + 2;
        while index < lines.len() {
            if lines[index].trim().is_empty() {
                break;
            }
            // Stop at lines that start other block constructs
            if is_thematic_break(&lines[index])
                || parse_fence_opener(&lines[index]).is_some()
                || strip_block_quote_marker(&lines[index]).is_some()
                || classify_html_block_start(&lines[index])
                    .is_some_and(|s| s.can_interrupt_paragraph)
            {
                break;
            }
            let line_range = trimmed_line_range(&lines[index]);
            let mut cells = split_table_row_spans(&lines[index]);
            // Body rows are normalised, not rejected: pad short rows with
            // empty cells and truncate long ones to the column count. A
            // padded cell has no source text, so it takes the empty span at
            // the row's end.
            // [spec:mw:req:core.node.table-cell-alignment]
            cells.resize(
                alignments.len(),
                (String::new(), line_range.end..line_range.end),
            );
            rows.push(Node::TableRow {
                cells: self.parse_table_cells(
                    &cells,
                    false,
                    &alignments,
                    row_offset,
                    refs,
                    footnote_labels,
                )?,
                source: SourceSpan::new(row_offset + line_range.start, row_offset + line_range.end),
            });
            row_offset += lines[index].len() + 1;
            index += 1;
        }

        let header_range = trimmed_line_range(&lines[start]);
        Ok(Some((
            index,
            ParsedBlock {
                node: Node::Table {
                    alignments: alignments.clone(),
                    header: vec![Node::TableRow {
                        cells: self.parse_table_cells(
                            &header_cells,
                            true,
                            &alignments,
                            0,
                            refs,
                            footnote_labels,
                        )?,
                        source: SourceSpan::new(header_range.start, header_range.end),
                    }],
                    rows,
                    source: SourceSpan::default(),
                },
                provisional: index == lines.len() && !finalizing && !terminated,
            },
        )))
    }

    // [spec:gfm:sem:leaf-blocks.tables-extension.spaces-around-cell-content-are-trimmed]
    // [spec:mw:req:core.node.table-cell-alignment]
    // [spec:mw:sem:core.span.unpopulated-is-empty+1]
    fn parse_table_cells(
        &self,
        cells: &[(String, core::ops::Range<usize>)],
        header: bool,
        alignments: &[Option<TableAlignment>],
        row_offset: usize,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<Vec<Node<Ext>>, Error>
    where
        Ext: Clone + PartialEq,
    {
        let mut out = Vec::with_capacity(cells.len());
        for (index, (cell, range)) in cells.iter().enumerate() {
            let start = row_offset + range.start;
            let mut node = Node::TableCell {
                children: self.parse_inline_nodes(cell.trim(), false, refs, footnote_labels)?,
                header,
                alignment: alignments.get(index).copied().flatten(),
                source: SourceSpan::new(start, row_offset + range.end),
            };
            offset_descendant_spans(&mut node, start);
            out.push(node);
        }
        Ok(out)
    }

    // [spec:gfm:def:leaf-blocks.fenced-code-blocks.code-fence-uses-backticks-or-tildes]
    // [spec:gfm:def:leaf-blocks.fenced-code-blocks.opening-fence-may-include-info-string]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.content-continues-until-matching-closing-fence]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.unclosed-block-continues-to-end-of-containing-block]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.simple-backtick-and-tilde-fences]
    // [spec:gfm:sem:leaf-blocks.fenced-code-blocks.content-is-literal-text]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.content-may-be-empty]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.content-may-consist-only-of-empty-lines]
    // [spec:gfm:sem:leaf-blocks.fenced-code-blocks.content-lines-without-indent-are-preserved]
    // [spec:gfm:sem:leaf-blocks.fenced-code-blocks.content-removes-up-to-opening-indent]
    // [spec:gfm:sem:leaf-blocks.fenced-code-blocks.short-indented-content-lines-remove-all-indent]
    // [spec:gfm:sem:leaf-blocks.fenced-code-blocks.fences-may-be-indented-and-content-deindents-to-match]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.unclosed-block-closes-at-end-of-containing-block]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.can-interrupt-paragraphs]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.can-be-followed-directly-by-paragraphs]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.other-blocks-can-appear-adjacent-without-blank-lines]
    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.info-string-may-be-provided-after-opening-fence]
    // [spec:gfm:sem:leaf-blocks.fenced-code-blocks.first-info-string-word-typically-identifies-language]
    // [spec:gfm:req:leaf-blocks.fenced-code-blocks.info-string-treatment-is-not-mandated]
    fn parse_fenced_code(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
    ) -> Option<(usize, Result<ParsedBlock<Ext>, Error>)>
    where
        Ext: Clone + PartialEq,
    {
        let opener = parse_fence_opener(&lines[start])?;
        let mut literal = String::new();
        let mut index = start + 1;
        let mut first_content = true;

        while index < lines.len() {
            if is_fence_closer(&lines[index], opener.marker, opener.count) {
                if !first_content {
                    literal.push('\n');
                }
                return Some((
                    index + 1,
                    Ok(ParsedBlock {
                        node: Node::CodeBlock {
                            info: opener.info,
                            literal,
                            source: SourceSpan::default(),
                        },
                        provisional: false,
                    }),
                ));
            }

            if !first_content {
                literal.push('\n');
            }
            first_content = false;
            literal.push_str(strip_up_to_n_leading_spaces(&lines[index], opener.indent));
            index += 1;
        }

        if !first_content {
            literal.push('\n');
        }
        Some((
            lines.len(),
            Ok(ParsedBlock {
                node: Node::CodeBlock {
                    info: opener.info,
                    literal,
                    source: SourceSpan::default(),
                },
                provisional: !finalizing,
            }),
        ))
    }

    // [spec:gfm:syn:leaf-blocks.atx-headings.structure]
    // [spec:gfm:syn:leaf-blocks.atx-headings.opening-sequence-length-one-to-six]
    // [spec:gfm:syn:leaf-blocks.atx-headings.opening-sequence-length-at-most-six]
    // [spec:gfm:syn:leaf-blocks.atx-headings.opening-sequence-followed-by-space-or-eol]
    // [spec:gfm:sem:leaf-blocks.atx-headings.contents-parse-as-inline-content]
    // [spec:gfm:syn:leaf-blocks.atx-headings.opening-indent-up-to-three-spaces]
    // [spec:gfm:syn:leaf-blocks.atx-headings.opening-indent-four-spaces-not-allowed]
    // [spec:gfm:syn:leaf-blocks.atx-headings.can-be-empty]
    // [spec:gfm:sem:leaf-blocks.atx-headings.trim-leading-and-trailing-whitespace-before-inline-parse]
    // [spec:gfm:syn:leaf-blocks.atx-headings.can-interrupt-paragraphs-without-blank-lines]
    // [spec:gfm:syn:leaf-blocks.atx-headings.escaped-opening-hash-does-not-open-heading]
    fn parse_atx_heading(
        &self,
        line: &str,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<Option<Node<Ext>>, Error>
    where
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
        let children = self.parse_inline_nodes(content, false, refs, footnote_labels)?;
        Ok(Some(Node::Heading {
            level: count as u8,
            children,
            source: SourceSpan::default(),
        }))
    }

    // [spec:gfm:syn:leaf-blocks.paragraphs.structure]
    // [spec:gfm:sem:leaf-blocks.paragraphs.contents-parse-as-inlines]
    // [spec:gfm:syn:leaf-blocks.paragraphs.may-span-multiple-lines-without-blank-lines]
    // [spec:gfm:syn:leaf-blocks.paragraphs.blank-line-separates-paragraphs]
    // [spec:gfm:syn:leaf-blocks.paragraphs.first-line-indent-up-to-three-spaces]
    // [spec:gfm:sem:leaf-blocks.paragraphs.leading-spaces-are-ignored]
    // [spec:gfm:syn:leaf-blocks.paragraphs.lines-after-first-may-be-indented-arbitrarily]
    // [spec:gfm:sem:leaf-blocks.paragraphs.multiple-blank-lines-between-paragraphs-have-no-effect]
    // [spec:gfm:sem:leaf-blocks.paragraphs.raw-content-concatenates-lines-and-trims-edge-whitespace]
    // [spec:gfm:sem:leaf-blocks.paragraphs.trailing-spaces-are-stripped-before-inline-parsing]
    // [spec:gfm:syn:leaf-blocks.setext-headings.structure]
    // [spec:gfm:sem:leaf-blocks.setext-headings.contents-parse-preceding-lines-as-inline-content]
    // [spec:gfm:syn:leaf-blocks.setext-headings.cannot-be-empty]
    // [spec:gfm:syn:leaf-blocks.setext-headings.cannot-interrupt-paragraphs]
    // [spec:gfm:syn:leaf-blocks.setext-headings.need-not-be-surrounded-by-blank-lines]
    // [spec:gfm:syn:leaf-blocks.setext-headings.blank-lines-generally-not-required-around-heading]
    // [spec:gfm:syn:leaf-blocks.setext-headings.blank-line-required-after-paragraph-before-heading]
    // [spec:gfm:syn:leaf-blocks.setext-headings.blank-lines-around-break-produce-hr]
    // [spec:gfm:syn:leaf-blocks.setext-headings.non-setext-break-marker-produces-hr]
    // [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-must-not-parse-as-other-block-constructs]
    // [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-must-otherwise-parse-as-paragraphs]
    // [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-require-non-whitespace-content]
    // [spec:gfm:syn:leaf-blocks.setext-headings.text-lines-indent-up-to-three-spaces]
    // [spec:gfm:syn:leaf-blocks.setext-headings.content-indent-up-to-three-spaces-and-need-not-align]
    // [spec:gfm:syn:leaf-blocks.setext-headings.content-indent-four-spaces-not-allowed]
    // [spec:gfm:sem:leaf-blocks.setext-headings.trailing-spaces-in-content-do-not-create-line-break]
    // [spec:gfm:sem:leaf-blocks.setext-headings.trailing-backslash-does-not-create-line-break]
    // [spec:gfm:syn:leaf-blocks.setext-headings.backslash-escaped-underline-produces-literal-text]
    // [spec:gfm:syn:leaf-blocks.setext-headings.backslash-escapes-allow-literal-block-markers-in-content]
    // [spec:gfm:syn:leaf-blocks.setext-headings.block-indicators-take-precedence-over-inline-indicators]
    // [spec:gfm:def:leaf-blocks.setext-headings.multiline-heading-has-four-interpretations]
    // [spec:gfm:sem:leaf-blocks.setext-headings.multiline-heading-interprets-as-heading-then-paragraph]
    // [spec:gfm:syn:leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs]
    // [spec:gfm:syn:leaf-blocks.html-blocks.type-7-cannot-interrupt-paragraphs]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.setext-underlines-convert-paragraphs]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-1-block-structure.reference-definitions-are-detected-at-paragraph-close]
    #[allow(clippy::too_many_arguments)]
    fn parse_paragraphish(
        &self,
        lines: &[String],
        start: usize,
        finalizing: bool,
        terminated: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
        lazy: &[bool],
    ) -> Result<(usize, ParsedBlock<Ext>), Error>
    where
        Ext: Clone + PartialEq,
    {
        let mut collected = vec![lines[start].clone()];
        let mut index = start + 1;

        while index < lines.len() {
            if lines[index].trim().is_empty() {
                break;
            }

            let is_lazy = lazy.get(index).copied().unwrap_or(false);

            // [spec:gfm:syn:leaf-blocks.thematic-breaks.setext-heading-takes-precedence-over-dash-break]
            // [spec:gfm:sem:leaf-blocks.setext-headings.level-determined-by-underline-marker]
            // [spec:gfm:def:leaf-blocks.setext-headings.underline-uses-equals-or-dashes]
            // [spec:gfm:syn:leaf-blocks.setext-headings.content-may-span-multiple-lines]
            // [spec:gfm:sem:leaf-blocks.setext-headings.raw-content-concatenates-lines-and-trims-edge-whitespace]
            // [spec:gfm:sem:leaf-blocks.setext-headings.contents-parse-preceding-lines-as-inline-content]
            // Setext underline cannot be a lazy continuation line
            if !is_lazy && is_setext_underline(&lines[index]) {
                let text = join_lines(&collected);
                let children =
                    self.parse_inline_nodes(text.trim(), false, refs, footnote_labels)?;
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
                            source: SourceSpan::default(),
                        },
                        provisional: false,
                    },
                ));
            }

            if parse_fence_opener(&lines[index]).is_some()
                || self
                    .parse_atx_heading(&lines[index], refs, footnote_labels)?
                    .is_some()
                || is_thematic_break(&lines[index])
                || strip_block_quote_marker(&lines[index]).is_some()
                || parse_list_marker(&lines[index]).is_some_and(can_interrupt_paragraph_with_list)
                || classify_html_block_start(&lines[index])
                    .is_some_and(|start| start.can_interrupt_paragraph)
            {
                break;
            }

            // GFM tables can interrupt a paragraph: current line is potential
            // header, next line is a delimiter row.
            if self.gfm
                && lines[index].contains('|')
                && index + 1 < lines.len()
                && parse_table_delimiter(&lines[index + 1]).is_some()
            {
                break;
            }

            collected.push(lines[index].clone());
            index += 1;
        }

        let text = join_lines(&collected);
        let trimmed = text.trim_ascii();
        let provisional =
            index == lines.len() && !finalizing && (terminated || !trimmed.is_empty());
        let children = self.parse_inline_nodes(trimmed, provisional, refs, footnote_labels)?;

        Ok((
            index,
            ParsedBlock {
                node: Node::Paragraph {
                    children,
                    source: SourceSpan::default(),
                },
                provisional,
            },
        ))
    }

    // [spec:gfm:thm:blocks-and-inlines.precedence.two-phase-parsing]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.phase-2-parses-inline-content]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.open-blocks-close-at-end-of-input]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.example-produces-softbreak-and-emphasis]
    fn parse_inline_nodes(
        &self,
        text: &str,
        provisional: bool,
        refs: &ReferenceDefinitions,
        footnote_labels: &FootnoteLabels,
    ) -> Result<Vec<Node<Ext>>, Error>
    where
        Ext: Clone + PartialEq,
    {
        let mut parser = InlineParser {
            input: text,
            index: 0,
            base_offset: 0,
            extensions: &self.extensions,
            provisional,
            refs,
            footnote_labels,
            gfm: self.gfm,
            math: self.math,
            highlights: self.highlights,
        };
        let nodes = parser.parse_until(None)?;
        Ok(promote_semantic_html(nodes))
    }
}

fn promote_semantic_html<Ext>(nodes: Vec<Node<Ext>>) -> Vec<Node<Ext>> {
    let mut result = Vec::with_capacity(nodes.len());
    let mut iter = nodes.into_iter().peekable();

    while let Some(node) = iter.next() {
        if let Some(tag) = semantic_open_tag(&node) {
            let close = format!("</{tag}>");
            let mut children = Vec::new();
            let mut found_close = false;

            for next in iter.by_ref() {
                if matches_html_close(&next, &close) {
                    found_close = true;
                    break;
                }
                children.push(next);
            }

            if found_close {
                let children = promote_semantic_html(children);
                let source = SourceSpan::default();
                let wrapped = match tag {
                    "sub" => Node::Subscript { children, source },
                    "sup" => Node::Superscript { children, source },
                    "mark" => Node::Highlight { children, source },
                    "del" => Node::Strikethrough { children, source },
                    _ => unreachable!(),
                };
                result.push(wrapped);
            } else {
                result.push(node);
                result.extend(children);
            }
        } else {
            result.push(node);
        }
    }

    result
}

fn semantic_open_tag<Ext>(node: &Node<Ext>) -> Option<&'static str> {
    let Node::Html { html, .. } = node else {
        return None;
    };
    let html = html.trim();
    for tag in &["sub", "sup", "mark", "del"] {
        let open = format!("<{tag}>");
        if html.eq_ignore_ascii_case(&open) {
            return Some(tag);
        }
    }
    None
}

fn matches_html_close<Ext>(node: &Node<Ext>, close: &str) -> bool {
    matches!(node, Node::Html { html, .. } if html.trim().eq_ignore_ascii_case(close))
}

fn is_definition_line(line: &str) -> Option<&str> {
    let trimmed = line.strip_prefix(':')?;
    let rest = trimmed
        .strip_prefix(' ')
        .or_else(|| trimmed.strip_prefix('\t'))?;
    Some(rest)
}

fn try_promote_alert<Ext>(children: &mut Vec<Node<Ext>>) -> Option<Node<Ext>>
where
{
    use crate::node::AlertKind;

    let first = children.first()?;
    let Node::Paragraph {
        children: inlines, ..
    } = first
    else {
        return None;
    };

    let first_inline = inlines.first()?;
    let Node::Text { text, .. } = first_inline else {
        return None;
    };

    let kind = if text.starts_with("[!NOTE]") {
        AlertKind::Note
    } else if text.starts_with("[!TIP]") {
        AlertKind::Tip
    } else if text.starts_with("[!IMPORTANT]") {
        AlertKind::Important
    } else if text.starts_with("[!WARNING]") {
        AlertKind::Warning
    } else if text.starts_with("[!CAUTION]") {
        AlertKind::Caution
    } else {
        return None;
    };

    let tag_len = match kind {
        AlertKind::Note => "[!NOTE]".len(),
        AlertKind::Tip => "[!TIP]".len(),
        AlertKind::Important => "[!IMPORTANT]".len(),
        AlertKind::Warning => "[!WARNING]".len(),
        AlertKind::Caution => "[!CAUTION]".len(),
    };

    let mut children = std::mem::take(children);
    let mut title: Option<String> = None;

    if let Some(Node::Paragraph {
        children: inlines, ..
    }) = children.first_mut()
    {
        if let Some(Node::Text { text, .. }) = inlines.first_mut() {
            // After `[!KIND]`, anything on the same line (before the first
            // newline) is an optional custom title; the remainder begins
            // the body of the alert.
            let after_tag = &text[tag_len..];
            let (title_candidate, rest_after_nl) = match after_tag.find('\n') {
                Some(nl) => (&after_tag[..nl], &after_tag[nl..]),
                None => (after_tag, ""),
            };
            let title_trimmed = title_candidate.trim();
            if !title_trimmed.is_empty() {
                title = Some(title_trimmed.to_string());
            }
            let rest = rest_after_nl.trim_start_matches('\n').to_string();
            if rest.is_empty() {
                inlines.remove(0);
                if let Some(Node::SoftBreak { .. }) = inlines.first() {
                    inlines.remove(0);
                }
            } else {
                *text = rest;
            }
        }
        if inlines.is_empty() {
            children.remove(0);
        }
    }

    Some(Node::Alert {
        kind,
        title,
        children,
        source: SourceSpan::default(),
    })
}

struct FenceOpener {
    marker: char,
    count: usize,
    info: Option<String>,
    indent: usize,
}

#[derive(Debug, Clone)]
struct ListMarker {
    ordered: bool,
    start: Option<u64>,
    marker_char: char,
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

// [spec:gfm:def:preliminaries.characters-and-lines.line-ending-definition]
// [spec:gfm:req:preliminaries.insecure-characters.null-character-is-replaced]
// [spec:gfm:sem:preliminaries.tabs.tabs-not-expanded-in-lines]
// [spec:gfm:def:preliminaries.characters-and-lines.character-definition]
// [spec:gfm:req:preliminaries.characters-and-lines.encoding-is-unspecified]
// [spec:gfm:syn:preliminaries.characters-and-lines.valid-commonmark-document]
pub(crate) fn normalize_chunk(chunk: &str) -> String {
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

// [spec:gfm:def:preliminaries.characters-and-lines.line-definition]
// [spec:gfm:def:preliminaries.characters-and-lines.blank-line-definition]
/// Add `offset` to the `source` span of every descendant of `node` (NOT the
/// node itself — its span is already absolute). Used to translate
/// inline-relative spans produced by `InlineParser` into source-absolute
/// coordinates.
fn offset_descendant_spans<Ext>(node: &mut Node<Ext>, offset: usize) {
    node.for_each_child_mut(|child| {
        let span = child.source();
        *child.source_mut() = SourceSpan::new(span.start + offset, span.end + offset);
        offset_descendant_spans(child, offset);
    });
}

/// Where a parse input's lines start, and where the input itself starts
/// within the document it was taken from. Turns a line index into a byte
/// offset in document coordinates.
#[derive(Clone, Copy)]
struct LineSpans<'a> {
    offsets: &'a [usize],
    base: usize,
}

impl LineSpans<'_> {
    /// Byte offset one past the last line of the input.
    fn eof(self) -> usize {
        self.offsets.last().copied().unwrap_or(0) + self.base
    }

    /// Byte offset the given line starts at; `eof` for a line past the end.
    fn start_of(self, line: usize) -> usize {
        self.offsets
            .get(line)
            .map_or_else(|| self.eof(), |offset| offset + self.base)
    }
}

/// Compute byte offsets where each line starts in `text`. Result has
/// `lines + 1` entries: the last is `text.len()` (exclusive end of the last
/// line). Line index `i`'s byte range is `result[i]..result[i+1]`.
fn compute_line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    if *offsets.last().unwrap() != text.len() {
        offsets.push(text.len());
    }
    offsets
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

// [spec:gfm:syn:leaf-blocks.link-reference-definitions.structure]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.label-followed-by-colon]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.label-indent-up-to-three-spaces]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.indent-four-spaces-prevents-definition]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.defines-label-for-reference-links-and-images]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-may-be-omitted]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-separated-from-destination-by-whitespace]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-must-be-separated-by-whitespace]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-allows-leading-whitespace]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.title-may-extend-over-multiple-lines]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.destination-allows-leading-whitespace]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.destination-and-title-allow-escapes]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.empty-destination-may-use-angle-brackets]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.link-destination-may-not-omitted]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.it-may-not-contain-blank-line]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.following-line-does-not-become-title]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.trailing-non-whitespace-invalidates-definition]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.no-non-whitespace-after-definition]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.multiline-label-form-does-not-define-reference]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.definition-status-is-independent-of-use]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.cannot-interrupt-paragraphs]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.may-follow-other-blocks-without-blank-line]
// [spec:gfm:sem:leaf-blocks.link-reference-definitions.may-occur-inside-block-containers]
// [spec:gfm:syn:leaf-blocks.link-reference-definitions.code-block-content-does-not-define-reference]
fn parse_reference_definition(
    lines: &[String],
    start: usize,
) -> Option<(usize, (String, ReferenceDefinition))> {
    // Join lines from `start` with newlines to handle multi-line definitions.
    // We stop at a blank line, which cannot appear inside a reference definition.
    let first_line = trim_up_to_three_spaces(&lines[start]);
    if !first_line.starts_with('[') {
        return None;
    }

    // Determine the text available for parsing: concatenate consecutive non-blank lines.
    let mut end_line = start;
    while end_line < lines.len() && !lines[end_line].trim().is_empty() {
        end_line += 1;
    }
    if end_line == start {
        return None;
    }

    // Build the joined text, tracking line boundaries to map back to `lines` index.
    // We use the stripped first line (without leading spaces) but keep original subsequent lines.
    let first_stripped_bytes = lines[start].len() - first_line.len();
    let mut text = String::from(first_line);
    let mut line_ends = vec![text.len()]; // byte position after each line (just before its \n)
    for line in &lines[(start + 1)..end_line] {
        text.push('\n');
        text.push_str(line);
        line_ends.push(text.len());
    }

    // Parse [label]
    let after_bracket = text.strip_prefix('[')?;
    let label_end = find_unescaped_bracket(after_bracket)?;
    let label = &after_bracket[..label_end];
    if label.trim().is_empty() || label.contains("\n\n") {
        return None;
    }
    // Label may span lines but must contain at least one non-whitespace character
    // `find_unescaped_bracket` already stopped at `]`; also disallow `[` inside
    // (which it doesn't track but spec forbids unescaped `[`).

    let after_label_abs = 1 + label_end + 1; // past the `]`
    let after_colon = text[after_label_abs..].strip_prefix(':')?;

    // Skip whitespace including up to one newline before destination.
    let (dest_start_rel, newlines_before_dest) = skip_ws_one_line(after_colon);
    if newlines_before_dest > 1 {
        return None;
    }
    let dest_abs = after_label_abs + 1 + dest_start_rel;
    let dest_text = &text[dest_abs..];
    if dest_text.is_empty() || dest_text.starts_with('\n') {
        return None;
    }

    // Parse destination
    let (destination, dest_consumed) = if let Some(body) = dest_text.strip_prefix('<') {
        let close = find_angle_close(body)?;
        let dest = process_escapes_and_entities(&body[..close]);
        (dest, 2 + close)
    } else {
        // Bare destination: up to first whitespace/control, with balanced parens
        let mut depth = 0usize;
        let mut escaped = false;
        let mut consumed = 0;
        for (i, ch) in dest_text.char_indices() {
            if escaped {
                escaped = false;
                consumed = i + ch.len_utf8();
                continue;
            }
            if ch == '\\' {
                escaped = true;
                consumed = i + 1;
                continue;
            }
            if ch == '(' {
                depth += 1;
                consumed = i + 1;
                continue;
            }
            if ch == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                consumed = i + 1;
                continue;
            }
            if ch.is_whitespace() || ch.is_ascii() && (ch as u8).is_ascii_control() {
                break;
            }
            consumed = i + ch.len_utf8();
        }
        if consumed == 0 || depth != 0 {
            return None;
        }
        (
            process_escapes_and_entities(&dest_text[..consumed]),
            consumed,
        )
    };

    let after_dest_abs = dest_abs + dest_consumed;

    // Look for a title. Skip whitespace including newlines (but not blank lines).
    // If the current line after destination has only whitespace, try next line for title.
    let remaining = &text[after_dest_abs..];
    let (title_start_rel, newlines_to_title) = skip_ws_one_line(remaining);
    let maybe_title_start = after_dest_abs + title_start_rel;

    // Determine the line index of destination end (for fallback case where title fails)
    let dest_end_line_idx = line_index_of(&line_ends, after_dest_abs);

    // Parse optional title
    let title: Option<String>;
    let end_abs: usize;

    if newlines_to_title > 1
        || maybe_title_start >= text.len()
        || (title_start_rel == 0 && newlines_to_title == 0)
    {
        title = None;
        end_abs = after_dest_abs;
    } else {
        let title_text = &text[maybe_title_start..];
        let first_char = title_text.chars().next().unwrap();
        let close_char_opt = match first_char {
            '"' => Some('"'),
            '\'' => Some('\''),
            '(' => Some(')'),
            _ => None,
        };

        if let Some(close_char) = close_char_opt {
            let body = &title_text[1..];
            let mut escaped = false;
            let mut close_at = None;
            let mut prev_was_newline = false;
            for (i, ch) in body.char_indices() {
                if escaped {
                    escaped = false;
                    prev_was_newline = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    prev_was_newline = false;
                    continue;
                }
                if ch == '\n' {
                    if prev_was_newline {
                        return None;
                    }
                    prev_was_newline = true;
                    continue;
                }
                if ch != ' ' && ch != '\t' {
                    prev_was_newline = false;
                }
                if ch == close_char {
                    close_at = Some(i);
                    break;
                }
                if first_char == '(' && ch == '(' {
                    return None;
                }
            }

            match close_at {
                Some(close_idx) => {
                    let title_body = &body[..close_idx];
                    title = Some(process_escapes_and_entities(title_body));
                    end_abs = maybe_title_start + 1 + close_idx + 1;
                }
                None => {
                    // Title didn't close; fall back to no title if dest line is clean
                    let trailing = &text[after_dest_abs..];
                    let trailing_on_dest_line =
                        trailing.find('\n').map_or(trailing, |nl| &trailing[..nl]);
                    if !trailing_on_dest_line.trim().is_empty() {
                        return None;
                    }
                    return Some((
                        dest_end_line_idx + 1,
                        (
                            normalize_reference_label(label),
                            ReferenceDefinition {
                                destination,
                                title: None,
                            },
                        ),
                    ));
                }
            }
        } else {
            // No title character; must be whitespace to end of line
            let trailing = &text[after_dest_abs..];
            if let Some(nl) = trailing.find('\n') {
                if !trailing[..nl].trim().is_empty() {
                    return None;
                }
                title = None;
                end_abs = after_dest_abs + nl;
            } else if trailing.trim().is_empty() {
                title = None;
                end_abs = after_dest_abs + trailing.len();
            } else {
                return None;
            }
        }
    }

    // After end_abs, rest of the line must be only whitespace
    let trailing = &text[end_abs..];
    let line_end_in_trailing = trailing.find('\n').unwrap_or(trailing.len());
    if !trailing[..line_end_in_trailing].trim().is_empty() {
        // If we had a title, we can't fall back because destination already consumed.
        // But if title was optional and we already committed, this is invalid.
        if title.is_some() {
            // Try again without the title: destination only, on its own line
            let trailing_after_dest = &text[after_dest_abs..];
            let nl = trailing_after_dest
                .find('\n')
                .unwrap_or(trailing_after_dest.len());
            if trailing_after_dest[..nl].trim().is_empty() {
                return Some((
                    dest_end_line_idx + 1,
                    (
                        normalize_reference_label(label),
                        ReferenceDefinition {
                            destination,
                            title: None,
                        },
                    ),
                ));
            }
        }
        return None;
    }

    // Determine how many source lines were consumed
    let end_line_idx = line_index_of(&line_ends, end_abs);
    let consumed_lines = end_line_idx + 1;

    let _ = first_stripped_bytes; // unused
    let normalized = normalize_reference_label(label);
    Some((
        start + consumed_lines,
        (normalized, ReferenceDefinition { destination, title }),
    ))
}

/// Skip spaces/tabs/newlines, but stop if more than one blank line.
/// Returns (bytes consumed, number of newlines passed).
fn skip_ws_one_line(s: &str) -> (usize, usize) {
    let mut i = 0;
    let mut newlines = 0usize;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' => i += 1,
            b'\n' => {
                newlines += 1;
                if newlines > 1 {
                    break;
                }
                i += 1;
            }
            _ => break,
        }
    }
    (i, newlines)
}

fn line_index_of(line_ends: &[usize], abs: usize) -> usize {
    for (i, &end) in line_ends.iter().enumerate() {
        if abs <= end {
            return i;
        }
    }
    line_ends.len().saturating_sub(1)
}

fn find_unescaped_bracket(input: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => return None,
            ']' => return Some(i),
            _ => {}
        }
    }
    None
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

enum MathFence {
    Dollar,
    Bracket,
}

fn math_block_opener(line: &str) -> Option<MathFence> {
    let trimmed = trim_up_to_three_spaces(line).trim();
    if trimmed == "$$" {
        Some(MathFence::Dollar)
    } else if trimmed == "\\[" {
        Some(MathFence::Bracket)
    } else {
        None
    }
}

fn math_block_closer(line: &str, opener: &MathFence) -> bool {
    let trimmed = trim_up_to_three_spaces(line).trim();
    match opener {
        MathFence::Dollar => trimmed == "$$",
        MathFence::Bracket => trimmed == "\\]",
    }
}

fn parse_math_block<Ext>(
    lines: &[String],
    start: usize,
    finalizing: bool,
) -> Option<(usize, ParsedBlock<Ext>)>
where
{
    let opener = math_block_opener(&lines[start])?;

    let mut content = Vec::new();
    let mut index = start + 1;
    while index < lines.len() {
        if math_block_closer(&lines[index], &opener) {
            index += 1;
            let math = content.join("\n");
            return Some((
                index,
                ParsedBlock {
                    node: Node::MathBlock {
                        math,
                        source: SourceSpan::default(),
                    },
                    provisional: false,
                },
            ));
        }
        content.push(lines[index].clone());
        index += 1;
    }

    // No closer found — if finalizing, include all remaining content
    if finalizing {
        let math = content.join("\n");
        Some((
            index,
            ParsedBlock {
                node: Node::MathBlock {
                    math,
                    source: SourceSpan::default(),
                },
                provisional: false,
            },
        ))
    } else {
        // Provisional — we might get a closer later
        let math = content.join("\n");
        Some((
            index,
            ParsedBlock {
                node: Node::MathBlock {
                    math,
                    source: SourceSpan::default(),
                },
                provisional: true,
            },
        ))
    }
}

fn trim_up_to_three_spaces(line: &str) -> &str {
    let mut start = 0;
    for (removed, (index, ch)) in line.char_indices().enumerate() {
        if removed == 3 || ch != ' ' {
            return &line[index..];
        }
        start = index + ch.len_utf8();
    }
    &line[start..]
}

// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-optional]
// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-length-independent]
// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-may-be-followed-by-spaces-only]
// [spec:gfm:syn:leaf-blocks.atx-headings.closing-sequence-must-be-preceded-by-space]
// [spec:gfm:syn:leaf-blocks.atx-headings.escaped-hash-does-not-count-in-closing-sequence]
// [spec:gfm:syn:leaf-blocks.atx-headings.non-space-after-closing-hashes-makes-them-content]
fn strip_atx_content(rest: &str) -> &str {
    let trimmed = rest.trim();
    let mut end = trimmed.len();
    let bytes = trimmed.as_bytes();

    while end > 0 && bytes[end - 1] == b'#' {
        end -= 1;
    }

    if end < trimmed.len() && end == 0 {
        // Content was only hashes (e.g. "### ###" where rest is "###")
        ""
    } else if end < trimmed.len() && end > 0 && bytes[end - 1] == b' ' {
        trimmed[..end].trim_end()
    } else {
        trimmed
    }
}

// [spec:gfm:sem:preliminaries.tabs.tab-after-block-quote-marker-counts-toward-indent]
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

// [spec:gfm:def:container-blocks.list-items.bullet-marker-definition]
// [spec:gfm:def:container-blocks.list-items.ordered-marker-definition]
// [spec:gfm:syn:container-blocks.list-items.at-least-one-space-required-after-marker]
// [spec:gfm:syn:container-blocks.list-items.ordered-start-number-is-at-most-nine-digits]
// [spec:gfm:syn:container-blocks.list-items.ordered-start-number-may-have-leading-zeroes]
// [spec:gfm:syn:container-blocks.list-items.empty-item-does-not-require-post-marker-spaces]
// [spec:gfm:sem:preliminaries.tabs.tab-indented-list-continuation-equivalence]
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
        let first_sep = rest.chars().next();
        if matches!(first_sep, Some(ch) if ch != ' ' && ch != '\t') {
            return None;
        }
        let base_col = indent.columns + marker_width;
        let (content, content_indent) = split_marker_content(rest, base_col, base_col + 1);
        return Some(ListMarker {
            ordered: false,
            start: None,
            marker_char: first,
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
    let first_sep = after_marker.chars().next();
    if matches!(first_sep, Some(ch) if ch != ' ' && ch != '\t') {
        return None;
    }

    let start = trimmed[..digit_count].parse().ok()?;
    let base_columns = indent.columns + marker_index + marker.len_utf8();
    let (content, content_indent) =
        split_marker_content(after_marker, base_columns, base_columns + 1);
    Some(ListMarker {
        ordered: true,
        start: Some(start),
        marker_char: marker,
        content,
        content_indent,
    })
}

/// Given the text following a list marker (e.g., after `-` or `1.`), determine
/// the content string and absolute content indent column. `base_col` is the
/// column just after the marker. `empty_col` is the fallback content_indent when
/// content is empty.
fn split_marker_content(after: &str, base_col: usize, empty_col: usize) -> (String, usize) {
    if after.is_empty() {
        return (String::new(), empty_col);
    }
    // Count whitespace columns between marker and content
    let mut col = base_col;
    let mut bytes = 0;
    for ch in after.chars() {
        let width = match ch {
            ' ' => 1,
            '\t' => indent_width(col, '\t'),
            _ => break,
        };
        col += width;
        bytes += ch.len_utf8();
    }

    // No content on this line
    if bytes == after.len() || after[bytes..].trim().is_empty() {
        return (String::new(), empty_col);
    }

    let post_spaces = col - base_col;

    // Expand the leading whitespace into spaces, accounting for tab stops
    // relative to the marker's absolute column position. This preserves
    // tab-stop alignment when subsequent code-block stripping happens.
    let mut expanded_indent = String::new();
    let mut col_walk = base_col;
    for ch in after.chars() {
        match ch {
            ' ' => {
                expanded_indent.push(' ');
                col_walk += 1;
            }
            '\t' => {
                let w = 4 - (col_walk % 4);
                for _ in 0..w {
                    expanded_indent.push(' ');
                }
                col_walk += w;
            }
            _ => break,
        }
    }

    // If indent after marker is 5+ spaces, treat as marker + 1 space + indented code
    if post_spaces >= 5 {
        // Drop just the first separator space; the rest stays as code indent.
        let mut rebuilt = String::from(&expanded_indent[1..]);
        rebuilt.push_str(&after[bytes..]);
        return (rebuilt, base_col + 1);
    }

    let content = after[bytes..].to_owned();
    (content, base_col + post_spaces)
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

// [spec:gfm:sem:preliminaries.tabs.tab-indented-code-block-equivalence]
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
    // Expand all leading tabs/spaces to spaces (tab stops at multiples of 4),
    // then strip exactly `columns` leading spaces.
    let mut expanded = String::new();
    let mut col = 0;
    let mut bytes_in = 0;
    for ch in line.chars() {
        match ch {
            ' ' => {
                expanded.push(' ');
                col += 1;
                bytes_in += 1;
            }
            '\t' => {
                let width = 4 - (col % 4);
                for _ in 0..width {
                    expanded.push(' ');
                }
                col += width;
                bytes_in += 1;
            }
            _ => break,
        }
    }
    if col < columns {
        return None;
    }
    let mut out = expanded[columns..].to_string();
    out.push_str(&line[bytes_in..]);
    Some(out)
}

fn strip_up_to_n_leading_spaces(line: &str, n: usize) -> &str {
    for (removed, (i, ch)) in line.char_indices().enumerate() {
        if removed >= n || ch != ' ' {
            return &line[i..];
        }
    }
    &line[line.len()..]
}

fn indent_width(current_columns: usize, ch: char) -> usize {
    match ch {
        ' ' => 1,
        '\t' => 4 - (current_columns % 4),
        _ => 0,
    }
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.opening-fence-indent-up-to-three-spaces]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.four-space-indented-fence-starts-indented-code-block]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.fewer-than-three-backticks-do-not-form-fence]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.fence-marker-cannot-mix-backticks-and-tildes]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.backtick-info-string-cannot-contain-backticks]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.backtick-info-strings-cannot-contain-backticks]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.tilde-info-strings-may-contain-backticks-and-tildes]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.fence-lines-cannot-contain-internal-spaces]
fn parse_fence_opener(line: &str) -> Option<FenceOpener> {
    let trimmed = trim_up_to_three_spaces(line);
    let indent = line.len() - trimmed.len();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    if count < 3 {
        return None;
    }

    let rest = &trimmed[count..];
    if marker == '`' && rest.contains('`') {
        return None;
    }

    // Per CommonMark: the info string is the full trimmed text after the
    // opening fence. Downstream consumers that only care about the
    // "language" (first word) can split themselves — this preserves
    // trailing attributes needed by custom rewriters (see
    // `CodeBlockRewriter`).
    let info_str = rest.trim();
    let info = (!info_str.is_empty()).then(|| process_escapes_and_entities(info_str));
    Some(FenceOpener {
        marker,
        count,
        info,
        indent,
    })
}

// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-length-at-least-opening-length]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-must-use-same-marker-type]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fences-cannot-have-info-strings]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-indent-up-to-three-spaces]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-indented-four-spaces-does-not-close]
// [spec:gfm:syn:leaf-blocks.fenced-code-blocks.closing-fence-indent-need-not-match-opening-indent]
fn is_fence_closer(line: &str, marker: char, count: usize) -> bool {
    let line = trim_up_to_three_spaces(line);
    let chars = line.chars();
    let run = chars.take_while(|ch| *ch == marker).count();
    run >= count && line[run..].trim().is_empty()
}

// [spec:gfm:def:leaf-blocks.thematic-breaks.structure]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.requires-dash-underscore-or-asterisk-markers]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.requires-three-or-more-marker-characters]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.all-marker-characters-must-match]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.spaces-or-tabs-allowed-between-markers]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.no-other-characters-allowed]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.marker-sequence-may-be-longer-than-three]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.trailing-spaces-allowed]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.indent-up-to-three-spaces]
// [spec:gfm:syn:leaf-blocks.thematic-breaks.indent-four-spaces-not-allowed]
fn is_thematic_break(line: &str) -> bool {
    let indent = leading_indent(line);
    if indent.columns > 3 {
        return false;
    }
    let line = &line[indent.bytes..];
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

// [spec:gfm:syn:leaf-blocks.setext-headings.underline-may-be-any-length]
// [spec:gfm:syn:leaf-blocks.setext-headings.underline-indent-up-to-three-spaces]
// [spec:gfm:syn:leaf-blocks.setext-headings.underline-indent-four-spaces-not-allowed]
// [spec:gfm:syn:leaf-blocks.setext-headings.setext-heading-underline-cannot-contain-internal-spaces]
// [spec:gfm:syn:leaf-blocks.setext-headings.underline-cannot-be-lazy-continuation-line]
fn is_setext_underline(line: &str) -> bool {
    let indent = leading_indent(line);
    if indent.columns > 3 {
        return false;
    }
    let trimmed = line[indent.bytes..].trim_end();
    if trimmed.is_empty() {
        return false;
    }

    let marker = trimmed.chars().next().unwrap_or(' ');
    if marker != '=' && marker != '-' {
        return false;
    }

    trimmed.chars().all(|ch| ch == marker)
}

struct InlineParser<'a, Ext> {
    input: &'a str,
    index: usize,
    /// Byte offset of `input` within the source document. Inline nodes get
    /// `source` spans relative to this base — e.g. for a paragraph whose
    /// content runs from byte 42 in source, `base_offset = 42` so an inline
    /// at `index = 5..10` gets `source = SourceSpan { start: 47, end: 52 }`.
    base_offset: usize,
    extensions: &'a [Box<dyn Extension<Ext>>],
    refs: &'a ReferenceDefinitions,
    footnote_labels: &'a FootnoteLabels,
    provisional: bool,
    gfm: bool,
    math: bool,
    highlights: bool,
}

impl<'a, Ext> InlineParser<'a, Ext> {
    /// Build a `SourceSpan` from the byte range `[start, self.index)` in
    /// `input`, translated to source coordinates via `base_offset`.
    // [spec:mw:req:core.span.sliceable]
    fn span_from(&self, start: usize) -> SourceSpan {
        SourceSpan::new(self.base_offset + start, self.base_offset + self.index)
    }
}

// [spec:mw:sem:core.ext.inline-context-isolation]
impl<'a, Ext> crate::ext::InlineContext<Ext> for InlineParser<'a, Ext>
where
    Ext: Clone + PartialEq,
{
    fn parse_inline(&self, source: &str) -> Vec<Node<Ext>> {
        let empty_refs: ReferenceDefinitions = ReferenceDefinitions::new();
        let empty_labels: FootnoteLabels = FootnoteLabels::new();
        let mut inner = InlineParser {
            input: source,
            index: 0,
            base_offset: 0,
            extensions: self.extensions,
            refs: &empty_refs,
            footnote_labels: &empty_labels,
            provisional: false,
            gfm: self.gfm,
            math: self.math,
            highlights: self.highlights,
        };
        match inner.parse_until(None) {
            Ok(nodes) => promote_semantic_html(nodes),
            Err(_) => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmphasisMarker {
    Asterisk,
    Underscore,
}

// [spec:gfm:def:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.delimiter-stack-definition]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.stack-entries-point-to-text-nodes]
#[derive(Debug, Clone, PartialEq, Eq)]
enum InlinePiece<Ext> {
    Text(String),
    Node(Node<Ext>),
    Delimiter {
        marker: EmphasisMarker,
        count: usize,
        can_open: bool,
        can_close: bool,
        /// Absolute byte position of the first delimiter character in the
        /// source (already includes the parser's `base_offset`). Used to
        /// build accurate `source` spans on the resulting `Emphasis` /
        /// `Strong` nodes when the delimiter run is consumed.
        byte_pos: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DelimiterRun {
    marker: EmphasisMarker,
    count: usize,
    can_open: bool,
    can_close: bool,
}

impl<'a, Ext> InlineParser<'a, Ext>
where
    Ext: Clone + PartialEq,
{
    // [spec:gfm:syn:inlines.inline-parsing-is-sequential]
    // [spec:gfm:syn:inlines.earlier-inline-match-takes-precedence-in-sequential-parse]
    // [spec:gfm:syn:inlines.backslash-escapes.any-ascii-punctuation-may-be-escaped]
    // [spec:gfm:syn:inlines.backslash-escapes.backslash-at-end-line-is-hard-line-break]
    // [spec:gfm:syn:inlines.backslash-escapes.backslashes-before-other-characters-are-literal]
    // [spec:gfm:syn:inlines.backslash-escapes.do-not-work-in-code-autolinks-or-raw-html]
    // [spec:gfm:syn:inlines.backslash-escapes.escaped-backslash-does-not-escape-following-character]
    // [spec:gfm:sem:inlines.backslash-escapes.escaped-characters-lose-special-meaning]
    // [spec:gfm:syn:inlines.backslash-escapes.work-in-urls-titles-references-and-info-strings]
    // [spec:gfm:def:inlines.code-spans.definition]
    // [spec:gfm:syn:inlines.code-spans.code-spans-take-precedence-over-emphasis-and-links]
    // [spec:gfm:syn:inlines.code-spans.code-spans-html-tags-and-autolinks-share-precedence]
    // [spec:gfm:def:inlines.hard-line-breaks.two-trailing-spaces-produce-hard-line-break]
    // [spec:gfm:syn:inlines.hard-line-breaks.backslash-before-line-ending-also-produces-hard-line-break]
    // [spec:gfm:syn:inlines.hard-line-breaks.hard-line-break-syntax-does-not-work-at-end-of-block]
    // [spec:gfm:syn:inlines.hard-line-breaks.hard-line-breaks-can-occur-inside-inline-constructs]
    // [spec:gfm:syn:inlines.hard-line-breaks.hard-line-breaks-do-not-occur-inside-code-spans-or-html-tags]
    // [spec:gfm:sem:inlines.hard-line-breaks.leading-spaces-on-next-line-are-ignored]
    // [spec:gfm:syn:inlines.hard-line-breaks.more-than-two-trailing-spaces-also-work]
    // [spec:gfm:def:inlines.soft-line-breaks.regular-line-break-produces-softbreak]
    // [spec:gfm:sem:inlines.soft-line-breaks.edge-spaces-around-softbreak-are-removed]
    // [spec:gfm:req:inlines.soft-line-breaks.renderers-may-optionally-promote-softbreaks]
    // [spec:gfm:req:inlines.soft-line-breaks.softbreak-may-render-as-line-ending-or-space]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-requires-matching-delimiters]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-emphasis-requires-matching-delimiters]
    // [spec:gfm:syn:inlines.textual-content.uninterpreted-characters-are-parsed-as-text]
    // [spec:gfm:sem:inlines.textual-content.internal-spaces-are-preserved]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.end-of-input-triggers-emphasis-processing]
    // [spec:mw:req:core.ext.inline-match-validity]
    fn parse_until(&mut self, stop: Option<&str>) -> Result<Vec<Node<Ext>>, Error> {
        let mut pieces = Vec::new();
        let mut text = String::new();

        while self.index < self.input.len() {
            if let Some(stop_delim) = stop
                && self.remaining().starts_with(stop_delim)
            {
                break;
            }

            let iter_start = self.index;
            let pieces_len_before = pieces.len();

            'iter: {
                if let Some(InlineMatch { consumed, node }) = self.try_extension() {
                    if consumed == 0
                        || self.index + consumed > self.input.len()
                        || !self.input.is_char_boundary(self.index + consumed)
                    {
                        return Err(Error::InvalidExtensionMatch {
                            location: Some(crate::error::ErrorLocation {
                                byte_offset: self.base_offset + self.index,
                            }),
                        });
                    }

                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(node));
                    self.index += consumed;
                    break 'iter;
                }

                // Math: \(...\) emits MathInline, \[...\] always emits MathBlock
                // (display math is block-level even when written inline). Must
                // precede the backslash-escape handler.
                if self.math
                    && (self.remaining().starts_with("\\(") || self.remaining().starts_with("\\["))
                    && let Some((math, display)) = self.try_math_backslash_span()
                {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(if display {
                        Node::MathBlock {
                            math,
                            source: SourceSpan::default(),
                        }
                    } else {
                        Node::MathInline {
                            math,
                            source: SourceSpan::default(),
                        }
                    }));
                    break 'iter;
                }

                if self.remaining().starts_with("\\\n") {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(Node::HardBreak {
                        source: SourceSpan::default(),
                    }));
                    self.index += 2;
                    while self.index < self.input.len()
                        && self.input.as_bytes().get(self.index) == Some(&b' ')
                    {
                        self.index += 1;
                    }
                    break 'iter;
                }

                if self.remaining().starts_with('\n') {
                    let trailing_spaces = text.chars().rev().take_while(|ch| *ch == ' ').count();
                    let is_hard = trailing_spaces >= 2;
                    if is_hard {
                        while text.ends_with(' ') {
                            text.pop();
                        }
                        flush_text_piece(&mut pieces, &mut text);
                        pieces.push(InlinePiece::Node(Node::HardBreak {
                            source: SourceSpan::default(),
                        }));
                    } else {
                        flush_text_piece(&mut pieces, &mut text);
                        pieces.push(InlinePiece::Node(Node::SoftBreak {
                            source: SourceSpan::default(),
                        }));
                    }
                    self.index += 1;
                    if is_hard {
                        while self.index < self.input.len()
                            && self.input.as_bytes().get(self.index) == Some(&b' ')
                        {
                            self.index += 1;
                        }
                    }
                    break 'iter;
                }

                if self.remaining().starts_with('`') {
                    if let Some(code) = self.try_code_span() {
                        flush_text_piece(&mut pieces, &mut text);
                        pieces.push(InlinePiece::Node(Node::CodeSpan {
                            code,
                            source: SourceSpan::default(),
                        }));
                        break 'iter;
                    }
                    // No matching closer — consume entire backtick run as literal text
                    let run = self.remaining().chars().take_while(|ch| *ch == '`').count();
                    for _ in 0..run {
                        text.push('`');
                    }
                    self.index += run;
                    break 'iter;
                }

                if self.math
                    && self.remaining().starts_with('$')
                    && let Some((math, display)) = self.try_math_span()
                {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(if display {
                        Node::MathBlock {
                            math,
                            source: SourceSpan::default(),
                        }
                    } else {
                        Node::MathInline {
                            math,
                            source: SourceSpan::default(),
                        }
                    }));
                    break 'iter;
                }

                if let Some(node) = self.try_autolink() {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(node));
                    break 'iter;
                }

                if self.gfm
                    && let Some(node) = self.try_autolink_literal()
                {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(node));
                    break 'iter;
                }

                if let Some(html) = self.try_html_inline() {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(Node::Html {
                        html,
                        source: SourceSpan::default(),
                    }));
                    break 'iter;
                }

                // If `<` looked like a URI autolink attempt (e.g., `<http://...`)
                // but failed (space inside, etc.), consume the whole `<...>` as
                // literal text so a bare URL autolink doesn't grab content within.
                if self.remaining().starts_with('<') {
                    let after_lt = &self.remaining()[1..];
                    if let Some(close) = after_lt.find('>') {
                        let body = &after_lt[..close];
                        if body.contains("://") || body.contains("http") {
                            text.push('<');
                            text.push_str(body);
                            text.push('>');
                            self.index += 1 + close + 1;
                            break 'iter;
                        }
                    }
                }

                if self.gfm
                    && let Some(node) = self.try_footnote_ref()
                {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(node));
                    break 'iter;
                }

                if let Some(node) = self.try_link_or_image()? {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Node(node));
                    break 'iter;
                }

                // GFM strikethrough: runs of 1 or 2 tildes (3+ are literal).
                if self.gfm && self.remaining().starts_with('~') {
                    let run = self.remaining().chars().take_while(|c| *c == '~').count();
                    if run <= 2 {
                        let delim: &str = if run == 2 { "~~" } else { "~" };
                        if let Some(node) = self.try_delimited(delim, false)? {
                            flush_text_piece(&mut pieces, &mut text);
                            let Node::Emphasis { children, .. } = node else {
                                unreachable!()
                            };
                            pieces.push(InlinePiece::Node(Node::Strikethrough {
                                children,
                                source: SourceSpan::default(),
                            }));
                            break 'iter;
                        }
                    }
                }

                if self.highlights
                    && self.remaining().starts_with("==")
                    && let Some(node) = self.try_delimited("==", false)?
                {
                    flush_text_piece(&mut pieces, &mut text);
                    let Node::Emphasis { children, .. } = node else {
                        unreachable!()
                    };
                    pieces.push(InlinePiece::Node(Node::Highlight {
                        children,
                        source: SourceSpan::default(),
                    }));
                    break 'iter;
                }

                if let Some(run) = self.parse_delimiter_run() {
                    flush_text_piece(&mut pieces, &mut text);
                    pieces.push(InlinePiece::Delimiter {
                        marker: run.marker,
                        count: run.count,
                        can_open: run.can_open,
                        can_close: run.can_close,
                        byte_pos: self.base_offset + self.index,
                    });
                    self.index += run.count;
                    break 'iter;
                }

                if self.remaining().starts_with('\\') {
                    let next_index = self.index + '\\'.len_utf8();
                    if next_index < self.input.len() {
                        let next = self.input[next_index..].chars().next().unwrap_or('\\');
                        if is_escapable(next) {
                            text.push(next);
                            self.index = next_index + next.len_utf8();
                            break 'iter;
                        }
                    }
                }

                if let Some((consumed, decoded)) = parse_entity_reference(self.remaining()) {
                    text.push_str(&decoded);
                    self.index += consumed;
                    break 'iter;
                }

                let next = self
                    .remaining()
                    .chars()
                    .next()
                    .ok_or(Error::MalformedEventStream {
                        location: Some(crate::error::ErrorLocation {
                            byte_offset: self.base_offset + self.index,
                        }),
                    })?;
                text.push(next);
                self.index += next.len_utf8();
            }

            // If a Node piece was just pushed, set its source span based on
            // the byte range we consumed during this iteration.
            if pieces.len() > pieces_len_before
                && let Some(InlinePiece::Node(node)) = pieces.last_mut()
            {
                *node.source_mut() = self.span_from(iter_start);
            }
        }

        flush_text_piece(&mut pieces, &mut text);
        Ok(resolve_emphasis(pieces))
    }

    // [spec:mw:req:core.ext.dispatch-order]
    fn try_extension(&self) -> Option<InlineMatch<Ext>> {
        for extension in self.extensions {
            if let Some(candidate) = extension.try_inline_ctx(self.remaining(), self) {
                return Some(candidate);
            }
        }
        None
    }

    // [spec:gfm:def:inlines.code-spans.backtick-string-definition]
    // [spec:gfm:syn:inlines.code-spans.opening-and-closing-backtick-strings-must-have-equal-length]
    // [spec:gfm:syn:inlines.code-spans.unclosed-backtick-string-is-literal]
    // [spec:gfm:syn:inlines.code-spans.longer-delimiters-allow-literal-backticks]
    // [spec:gfm:syn:inlines.code-spans.example]
    // [spec:gfm:sem:inlines.code-spans.line-endings-become-spaces]
    // [spec:gfm:sem:inlines.code-spans.line-endings-conversion-example]
    // [spec:gfm:sem:inlines.code-spans.interior-spaces-are-preserved]
    // [spec:gfm:sem:inlines.code-spans.single-edge-space-is-trimmed]
    // [spec:gfm:sem:inlines.code-spans.edge-space-trimming-requires-space-on-both-sides]
    // [spec:gfm:sem:inlines.code-spans.edge-space-trimming-allows-literal-backticks-at-edges]
    // [spec:gfm:sem:inlines.code-spans.only-one-space-is-trimmed]
    // [spec:gfm:sem:inlines.code-spans.only-ascii-space-is-trimmed]
    // [spec:gfm:sem:inlines.code-spans.all-space-content-is-not-trimmed]
    // [spec:gfm:thm:inlines.code-spans.backslash-escapes-are-never-needed]
    // [spec:gfm:syn:inlines.code-spans.backslashes-are-literal]
    // [spec:gfm:req:inlines.code-spans.browser-rendering-may-collapse-spaces+1]
    // [spec:gfm:syn:inlines.code-spans.autolink-wins-when-autolink-parses-first]
    // [spec:gfm:syn:inlines.code-spans.html-tag-wins-when-tag-parses-first]
    // [spec:gfm:syn:inlines.code-spans.code-span-prevents-link-parse]
    // [spec:gfm:syn:inlines.code-spans.code-span-wins-in-ambiguous-autolink-like-text]
    fn try_code_span(&mut self) -> Option<String> {
        let remaining = self.remaining();
        if !remaining.starts_with('`') {
            return None;
        }

        let run = remaining.chars().take_while(|ch| *ch == '`').count();
        let search_start = self.index + run;
        let rest = &self.input[search_start..];

        // Find a run of backticks of EXACTLY `run` length
        let bytes = rest.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                let j = i + bytes[i..].iter().take_while(|&&b| b == b'`').count();
                if j - i == run {
                    let end = search_start + i;
                    let code = self.input[search_start..end].replace('\n', " ");
                    self.index = end + run;
                    return Some(normalize_code_span(&code));
                }
                i = j;
            } else {
                i += 1;
            }
        }

        None
    }

    fn try_math_span(&mut self) -> Option<(String, bool)> {
        let remaining = self.remaining();
        let (run, display) = if remaining.starts_with("$$") {
            (2, true)
        } else if remaining.starts_with('$') {
            (1, false)
        } else {
            return None;
        };

        let search_start = self.index + run;
        let rest = &self.input[search_start..];

        if display {
            let close = rest.find("$$")?;
            let content = &rest[..close];
            self.index = search_start + close + 2;
            return Some((content.to_string(), true));
        }

        // Single $: find matching $ that isn't part of $$
        let bytes = rest.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'$' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                    i += 2;
                    continue;
                }
                // Check backslash escape
                if i > 0 && bytes[i - 1] == b'\\' {
                    i += 1;
                    continue;
                }
                let content = &rest[..i];
                // Content must not start AND end with a space (unless all spaces)
                if content.len() > 1
                    && content.starts_with(' ')
                    && content.ends_with(' ')
                    && !content.trim().is_empty()
                {
                    i += 1;
                    continue;
                }
                if content.is_empty() {
                    i += 1;
                    continue;
                }
                self.index = search_start + i + 1;
                return Some((content.to_string(), false));
            }
            i += 1;
        }
        None
    }

    fn try_math_backslash_span(&mut self) -> Option<(String, bool)> {
        let remaining = self.remaining();
        if remaining.starts_with("\\(") {
            let search_start = self.index + 2;
            let rest = &self.input[search_start..];
            let close = rest.find("\\)")?;
            let content = &rest[..close];
            if content.is_empty() {
                return None;
            }
            self.index = search_start + close + 2;
            Some((content.to_string(), false))
        } else if remaining.starts_with("\\[") {
            let search_start = self.index + 2;
            let rest = &self.input[search_start..];
            let close = rest.find("\\]")?;
            let content = &rest[..close];
            if content.is_empty() {
                return None;
            }
            self.index = search_start + close + 2;
            Some((content.to_string(), true))
        } else {
            None
        }
    }

    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.delimiter-run-definition]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.delimiter-run-example]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.left-flanking-delimiter-run-definition]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.right-flanking-delimiter-run-definition]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.left-right-flanking-terminology-note]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.single-asterisk-can-open-emphasis]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.single-asterisk-can-close-emphasis]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.single-underscore-can-open-emphasis]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.single-underscore-can-close-emphasis]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.double-asterisk-can-open-strong-emphasis]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.double-asterisk-can-close-strong-emphasis]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-open-strong-emphasis]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-close-strong-emphasis]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-can-open-when-punctuation-precedes]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-can-close-when-punctuation-follows]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-open-when-punctuation-precedes]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-can-close-when-punctuation-follows]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-when-followed-by-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-after-alphanumeric-before-punctuation]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-punctuation-before-alphanumeric]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.asterisk-closing-restriction-example]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-alphanumeric-before-punctuation]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-punctuation-before-alphanumeric]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-alphanumeric-before-punctuation]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-punctuation-before-alphanumeric]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-asterisk-closing-restriction-examples]
    // [spec:gfm:thm:inlines.emphasis-and-strong-emphasis.double-asterisk-closing-cannot-fall-back-to-single-asterisk-rule]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-whitespace]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-alphanumeric-before-punctuation]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-punctuation-before-alphanumeric]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.double-underscore-closing-restriction-example]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-closing]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-double-underscore]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.right-flanking-underscore-does-not-emphasize]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-allowed]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-disallowed]
    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-double-underscore-closing-is-forbidden]
    // [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.many-implementations-restrict-intraword-underscore]
    // [spec:gfm:def:inlines.emphasis-and-strong-emphasis.unicode-nonbreaking-space-counts-as-whitespace]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.push-delimiters-to-stack]
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

    // [spec:gfm:def:inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
    // [spec:gfm:syn:inlines.strikethrough-extension.new-paragraph-ends-strikethrough-parse]
    // [spec:gfm:def:inlines.strikethrough-extension.gfm-enables-strikethrough-extension]
    fn try_delimited(&mut self, delim: &str, strong: bool) -> Result<Option<Node<Ext>>, Error> {
        let remaining = self.remaining();
        if !remaining.starts_with(delim) {
            return Ok(None);
        }

        if (delim == "~~" || delim == "~")
            && !is_valid_tilde_opener(self.input, self.index, delim.len())
        {
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
            base_offset: self.base_offset + inner_start,
            extensions: self.extensions,
            refs: self.refs,
            footnote_labels: self.footnote_labels,
            provisional: self.provisional,
            gfm: self.gfm,
            math: self.math,
            highlights: self.highlights,
        };
        let children = inner.parse_until(None)?;
        self.index = inner_end + delim.len();

        if strong {
            Ok(Some(Node::Strong {
                children,
                source: SourceSpan::default(),
            }))
        } else if self.index != saved + delim.len() {
            Ok(Some(Node::Emphasis {
                children,
                source: SourceSpan::default(),
            }))
        } else {
            Ok(None)
        }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.index..]
    }

    // [spec:gfm:def:inlines.links.definition]
    // [spec:gfm:syn:inlines.links.inline-link-definition]
    // [spec:gfm:sem:inlines.links.inline-link-example]
    // [spec:gfm:def:inlines.links.inline-link-text-comes-from-link-text-inlines]
    // [spec:gfm:sem:inlines.links.inline-link-uri-comes-from-destination-with-delimiters-removed]
    // [spec:gfm:sem:inlines.links.inline-link-title-comes-from-title-with-delimiters-removed]
    // [spec:gfm:def:inlines.links.inline-and-reference-links-are-the-two-basic-kinds]
    // [spec:gfm:syn:inlines.links.inline-links-take-precedence-over-reference-links]
    // [spec:gfm:def:inlines.links.full-reference-link-definition]
    // [spec:gfm:sem:inlines.links.full-reference-link-example]
    // [spec:gfm:syn:inlines.links.full-and-collapsed-references-take-precedence-over-shortcuts]
    // [spec:gfm:syn:inlines.links.collapsed-reference-link-definition]
    // [spec:gfm:sem:inlines.links.collapsed-reference-link-is-equivalent-to-full-link-with-same-label]
    // [spec:gfm:sem:inlines.links.collapsed-reference-link-labels-are-case-insensitive]
    // [spec:gfm:def:inlines.links.collapsed-reference-link-text-comes-from-first-label-inlines]
    // [spec:gfm:sem:inlines.links.no-whitespace-allowed-between-collapsed-reference-brackets]
    // [spec:gfm:syn:inlines.links.shortcut-reference-link-definition]
    // [spec:gfm:sem:inlines.links.shortcut-reference-link-is-equivalent-to-collapsed-reference-link]
    // [spec:gfm:sem:inlines.links.shortcut-reference-link-labels-are-case-insensitive]
    // [spec:gfm:def:inlines.links.shortcut-reference-link-text-comes-from-label-inlines]
    // [spec:gfm:syn:inlines.links.shortcut-reference-does-not-parse-when-followed-by-link-label]
    // [spec:gfm:sem:inlines.links.shortcut-reference-links-make-space-before-label-too-dangerous]
    // [spec:gfm:sem:inlines.links.space-after-shortcut-reference-link-is-preserved]
    // [spec:gfm:syn:inlines.links.reference-links-have-three-kinds]
    // [spec:gfm:syn:inlines.links.reference-link-text-allows-balanced-or-escaped-brackets]
    // [spec:gfm:syn:inlines.links.reference-link-text-cannot-contain-links]
    // [spec:gfm:sem:inlines.links.reference-link-text-comes-from-first-label-inlines]
    // [spec:gfm:syn:inlines.links.reference-link-text-follows-inline-link-text-rules]
    // [spec:gfm:syn:inlines.links.reference-link-text-may-contain-inline-content]
    // [spec:gfm:syn:inlines.links.reference-link-text-grouping-takes-precedence-over-emphasis]
    // [spec:gfm:syn:inlines.links.reference-link-may-parse-later-brackets-as-separate-text]
    // [spec:gfm:syn:inlines.links.reference-links-lose-to-html-tags-code-spans-and-autolinks]
    // [spec:gfm:def:inlines.links.first-matching-reference-definition-is-used]
    // [spec:gfm:sem:inlines.links.first-reference-definition-wins-example]
    // [spec:gfm:syn:inlines.links.defined-middle-label-makes-earlier-reference-parse]
    // [spec:gfm:syn:inlines.links.nested-reference-link-examples-produce-shortcuts]
    // [spec:gfm:sem:inlines.links.escaping-opening-bracket-prevents-reference-link]
    // [spec:gfm:def:inlines.links.link-text-definition]
    // [spec:gfm:syn:inlines.links.link-text-may-contain-inline-content]
    // [spec:gfm:syn:inlines.links.link-text-may-contain-balanced-brackets-but-not-unbalanced-ones]
    // [spec:gfm:syn:inlines.links.link-text-allows-only-escaped-or-balanced-brackets]
    // [spec:gfm:def:inlines.links.link-text-cannot-contain-links]
    // [spec:gfm:syn:inlines.links.link-text-brackets-bind-more-tightly-than-emphasis]
    // [spec:gfm:syn:inlines.links.links-may-not-contain-other-links-at-any-level-nesting]
    // [spec:gfm:syn:inlines.links.brackets-aren-t-part-links-do-not-take-precedence]
    // [spec:gfm:syn:inlines.links.code-autolink-and-html-bind-tighter-than-link-text]
    // [spec:gfm:syn:inlines.links.cases-illustrate-precedence-html-tags-code-spans-autolinks-over-link]
    // [spec:gfm:syn:inlines.links.cases-illustrate-precedence-link-text-grouping-over-emphasis-grouping]
    // [spec:gfm:syn:inlines.links.destination-and-title-may-both-be-omitted]
    // [spec:gfm:syn:inlines.links.no-whitespace-allowed-between-link-text-and-opening-parenthesis]
    // [spec:gfm:sem:inlines.links.no-whitespace-allowed-between-link-text-and-link-label]
    // [spec:gfm:syn:inlines.links.no-space-before-link-label-departs-from-original-markdown]
    // [spec:gfm:sem:inlines.links.whitespace-is-allowed-inside-inline-link-delimiters]
    // [spec:gfm:sem:inlines.links.matching-uses-normalized-strings-not-parsed-inline-content]
    // [spec:gfm:syn:inlines.links.markdown-pl-title-behavior]
    // [spec:gfm:syn:inlines.images.definition]
    // [spec:gfm:sem:inlines.images.html-rendering-should-use-plain-string-content-for-alt]
    // [spec:gfm:def:inlines.images.image-description-follows-link-text-rules-with-two-differences]
    // [spec:gfm:syn:inlines.images.image-description-contains-inline-elements]
    // [spec:gfm:syn:inlines.images.escaping-bang-produces-literal-bang-followed-by-link]
    // [spec:gfm:sem:inlines.images.escaping-opening-bracket-after-bang-produces-literal-bracketed-text]
    // [spec:gfm:sem:inlines.images.no-whitespace-allowed-between-reference-image-brackets]
    // [spec:gfm:req:inlines.images.reference-image-labels-are-case-insensitive]
    // [spec:gfm:syn:inlines.images.reference-image-labels-cannot-contain-unescaped-brackets]
    // [spec:gfm:syn:inlines.images.shortcut-image-labels-are-case-insensitive]
    // GFM footnotes: [^label] references a footnote definition collected earlier.
    // Only emit a FootnoteRef if a matching definition exists; otherwise fall
    // through so the text becomes literal (matching cmark-gfm behaviour).
    fn try_footnote_ref(&mut self) -> Option<Node<Ext>> {
        let remaining = self.remaining();
        if !remaining.starts_with("[^") {
            return None;
        }
        let body = &remaining[2..];
        let end = find_footnote_label_end(body)?;
        let label = &body[..end];
        if label.is_empty() || label.chars().any(|c| c.is_whitespace()) {
            return None;
        }
        if !self.footnote_labels.contains(label) {
            return None;
        }
        self.index += 2 + end + 1;
        Some(Node::FootnoteRef {
            label: label.to_string(),
            number: 0,
            ref_num: 0,
            source: SourceSpan::default(),
        })
    }

    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.closing-bracket-triggers-link-search]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-scan-backward]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-failure-case]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-builds-node-children]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-removes-opener+1]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-deactivates-earlier-openers]
    // [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-runs-emphasis-processing]
    fn try_link_or_image(&mut self) -> Result<Option<Node<Ext>>, Error> {
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

        if let Some(after_open) = after.strip_prefix('(')
            && let Some((consumed, destination, title)) = parse_inline_link_target(after_open)
        {
            let children = parse_inline_children(
                label,
                self.extensions,
                self.refs,
                self.footnote_labels,
                self.provisional,
                self.gfm,
                self.math,
                self.highlights,
            )?;
            // Links cannot contain other links
            if !image && contains_link(&children) {
                return Ok(None);
            }
            self.index = label_end + 2 + consumed;
            return Ok(Some(build_link_or_image(
                image,
                destination,
                title,
                children,
                SourceSpan::default(),
            )));
        }
        // Inline link failed; fall through to try reference link matching.

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
            self.extensions,
            self.refs,
            self.footnote_labels,
            self.provisional,
            self.gfm,
            self.math,
            self.highlights,
        )?;
        if !image && contains_link(&children) {
            return Ok(None);
        }
        self.index = label_end + 1 + consumed_after;
        Ok(Some(build_link_or_image(
            image,
            def.destination.clone(),
            def.title.clone(),
            children,
            SourceSpan::default(),
        )))
    }

    // [spec:gfm:def:inlines.raw-html.html-like-tag-text-is-parsed-as-raw-html]
    // [spec:gfm:def:inlines.raw-html.html-tag-definition]
    // [spec:gfm:syn:inlines.raw-html.tag-grammar-follows]
    // [spec:gfm:syn:inlines.raw-html.simple-open-tag-examples]
    // [spec:gfm:syn:inlines.raw-html.custom-tag-names-are-allowed]
    // [spec:gfm:syn:inlines.raw-html.custom-tag-and-attribute-names-are-allowed]
    // [spec:gfm:syn:inlines.raw-html.closing-tags-cannot-have-attributes]
    // [spec:gfm:syn:inlines.raw-html.illegal-tag-and-attribute-syntax-is-not-parsed-as-html]
    // [spec:gfm:def:inlines.raw-html.html-comment-definition]
    // [spec:gfm:def:inlines.raw-html.processing-instruction-definition]
    // [spec:gfm:def:inlines.raw-html.declaration-definition]
    // [spec:gfm:def:inlines.raw-html.cdata-section-definition]
    // [spec:gfm:syn:inlines.raw-html.backslash-escapes-do-not-work-in-html-attributes]
    // [spec:gfm:sem:inlines.raw-html.entity-and-numeric-references-are-preserved-in-html-attributes]
    // [spec:gfm:def:inlines.raw-html.attribute-value-definition]
    // TODO: disallowed raw HTML extension is not yet implemented
    // [spec:gfm:def:inlines.disallowed-raw-html-extension.disallowed-tags]
    // [spec:gfm:sem:inlines.disallowed-raw-html-extension.filtering-replaces-leading-angle-bracket]
    // [spec:gfm:sem:inlines.disallowed-raw-html-extension.disallowed-tags-change-html-parsing]
    // [spec:gfm:sem:inlines.disallowed-raw-html-extension.all-other-html-tags-are-left-untouched]
    // [spec:gfm:def:inlines.disallowed-raw-html-extension.gfm-enables-tagfilter-extension]
    fn try_html_inline(&mut self) -> Option<String> {
        let remaining = self.remaining();
        if !remaining.starts_with('<') {
            return None;
        }

        let end = if remaining.starts_with("<!--") {
            remaining.find("-->").map(|idx| idx + 3)?
        } else if remaining.starts_with("<?") {
            remaining.find("?>").map(|idx| idx + 2)?
        } else if remaining.starts_with("<![CDATA[") {
            remaining.find("]]>").map(|idx| idx + 3)?
        } else if remaining.starts_with("<!")
            && remaining
                .as_bytes()
                .get(2)
                .is_some_and(|b| b.is_ascii_uppercase())
        {
            remaining.find('>').map(|idx| idx + 1)?
        } else if let Some((_, len)) = parse_html_open_tag_prefix(remaining) {
            len
        } else if let Some((_, len)) = parse_html_closing_tag_prefix(remaining) {
            len
        } else {
            return None;
        };

        let html = if self.gfm {
            filter_disallowed_html(&remaining[..end])
        } else {
            remaining[..end].to_string()
        };
        self.index += end;
        Some(html)
    }

    // [spec:gfm:syn:inlines.autolinks.definition]
    // [spec:gfm:def:inlines.autolinks.uri-autolink-definition]
    // [spec:gfm:syn:inlines.autolinks.email-autolink-definition]
    // [spec:gfm:def:inlines.autolinks.absolute-uri-definition]
    // [spec:gfm:def:inlines.autolinks.scheme-definition]
    // [spec:gfm:syn:inlines.autolinks.autolink-schemes-are-case-insensitive]
    // [spec:gfm:syn:inlines.autolinks.autolinks-cannot-contain-spaces]
    // [spec:gfm:syn:inlines.autolinks.backslash-escapes-do-not-work-inside-autolinks]
    // [spec:gfm:syn:inlines.autolinks.backslash-escapes-do-not-work-inside-email-autolinks]
    // [spec:gfm:def:inlines.autolinks.email-address-definition]
    // [spec:gfm:def:inlines.autolinks.email-address-regex]
    // [spec:gfm:syn:inlines.autolinks.invalid-autolink-example]
    // [spec:gfm:syn:inlines.autolinks.spec-accepts-some-absolute-uris-that-are-not-valid-uris]
    // [spec:gfm:def:inlines.autolinks.valid-email-autolink-example]
    // [spec:gfm:def:inlines.autolinks.valid-uri-autolink-example]
    fn try_autolink(&mut self) -> Option<Node<Ext>> {
        let remaining = self.remaining();
        if !remaining.starts_with('<') {
            return None;
        }
        let close = remaining.find('>')?;
        let body = &remaining[1..close];
        if body.contains(' ') || body.contains('\n') {
            return None;
        }
        if looks_like_url(body) {
            self.index += close + 1;
            return Some(Node::Link {
                destination: body.to_owned(),
                title: None,
                children: vec![Node::Text {
                    text: body.to_owned(),
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
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
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
            });
        }
        None
    }

    // [spec:gfm:def:inlines.autolinks-extension.definition]
    // [spec:gfm:syn:inlines.autolinks-extension.extended-url-autolink-definition]
    // [spec:gfm:def:inlines.autolinks-extension.extended-email-autolink-definition]
    // [spec:gfm:syn:inlines.autolinks-extension.extended-www-autolink-definition]
    // [spec:gfm:def:inlines.autolinks-extension.extended-www-autolinks-insert-http-scheme]
    // [spec:gfm:sem:inlines.autolinks-extension.extended-www-autolinks-may-have-path-after-domain]
    // [spec:gfm:def:inlines.autolinks-extension.extended-email-autolink-rules]
    // [spec:gfm:syn:inlines.autolinks-extension.extended-email-autolinks-insert-mailto-scheme]
    // [spec:gfm:syn:inlines.autolinks-extension.extended-autolink-path-validation]
    // [spec:gfm:def:inlines.autolinks-extension.gfm-enables-extended-autolinks]
    // [spec:gfm:syn:inlines.autolinks-extension.left-angle-bracket-ends-autolink]
    // [spec:gfm:def:inlines.autolinks-extension.trailing-punctuation-is-excluded-from-autolinks]
    // [spec:gfm:syn:inlines.autolinks-extension.trailing-semicolon-is-excluded-when-it-looks-like-entity]
    // [spec:gfm:syn:inlines.autolinks-extension.trailing-parenthesis-check-applies-only-at-link-end]
    // [spec:gfm:syn:inlines.autolinks-extension.unmatched-trailing-right-parentheses-are-excluded]
    // [spec:gfm:syn:inlines.autolinks-extension.dot-dash-and-underscore-rules-around-at-sign]
    // [spec:gfm:sem:inlines.autolinks-extension.plus-is-allowed-before-but-not-after-at-sign]
    fn try_autolink_literal(&mut self) -> Option<Node<Ext>> {
        let remaining = self.remaining();

        // GFM extended autolinks for `mailto:` and `xmpp:` schemes.
        // Must follow a word boundary (start of input, whitespace, or punctuation
        // that's not part of an alphanumeric word).
        for scheme in ["mailto:", "xmpp:"] {
            if let Some(after) = remaining.strip_prefix(scheme) {
                // Word-boundary check: previous char must not be alphanumeric
                let prev = char_before(self.input, self.index);
                if prev.is_some_and(|c| c.is_ascii_alphanumeric()) {
                    continue;
                }
                // Find end at whitespace or `<`
                let end = after
                    .find(|ch: char| ch.is_whitespace() || matches!(ch, '<'))
                    .unwrap_or(after.len());
                let body = &after[..end];
                // The body must contain `@` for it to be an email-style autolink
                if !body.contains('@') {
                    continue;
                }
                // The email part is up to the first `/` regardless of scheme;
                // for xmpp we then optionally include `/resource` after it.
                let email_end_in_body = body.find('/').unwrap_or(body.len());
                let email_part = &body[..email_end_in_body];
                let validated = email_part.trim_end_matches('.');
                if !looks_like_email(validated) {
                    continue;
                }
                let total_in_body = if scheme == "xmpp:" {
                    body.len()
                } else {
                    email_part.len()
                };
                let full_text = &remaining[..scheme.len() + total_in_body];
                let trimmed_full = full_text.trim_end_matches('.');
                let trimmed_full = trim_autolink_trailing(trimmed_full);
                self.index += trimmed_full.len();
                return Some(Node::Link {
                    destination: trimmed_full.to_owned(),
                    title: None,
                    children: vec![Node::Text {
                        text: trimmed_full.to_owned(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                });
            }
        }

        for prefix in ["https://", "http://", "ftp://"] {
            if let Some(rest) = remaining.strip_prefix(prefix) {
                let end = rest
                    .find(|ch: char| ch.is_whitespace() || matches!(ch, '<'))
                    .unwrap_or(rest.len());
                let raw = format!("{prefix}{}", &rest[..end]);
                let trimmed = trim_autolink_trailing(&raw);
                self.index += trimmed.len();
                return Some(Node::Link {
                    destination: trimmed.to_owned(),
                    title: None,
                    children: vec![Node::Text {
                        text: trimmed.to_owned(),
                        source: SourceSpan::default(),
                    }],
                    source: SourceSpan::default(),
                });
            }
        }

        if remaining.starts_with("www.") {
            let end = remaining
                .find(|ch: char| ch.is_whitespace() || matches!(ch, '<'))
                .unwrap_or(remaining.len());
            let raw = &remaining[..end];
            // Validate domain: underscores not allowed in last 2 segments of host
            if !is_valid_www_host(raw) {
                return None;
            }
            let trimmed = trim_autolink_trailing(raw);
            self.index += trimmed.len();
            let destination = format!("http://{trimmed}");
            return Some(Node::Link {
                destination,
                title: None,
                children: vec![Node::Text {
                    text: trimmed.to_owned(),
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
            });
        }

        // Bare email autolinks: stop at whitespace, `<`, or `/`
        // (path components after the email aren't part of the autolink).
        let end = remaining
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '/'))
            .unwrap_or(remaining.len());
        let candidate = &remaining[..end];
        // For emails, only strip trailing dots (not full URL trailing rules)
        let trimmed_email = candidate.trim_end_matches('.');
        if !trimmed_email.is_empty() && looks_like_email(trimmed_email) {
            self.index += trimmed_email.len();
            return Some(Node::Link {
                destination: format!("mailto:{trimmed_email}"),
                title: None,
                children: vec![Node::Text {
                    text: trimmed_email.to_owned(),
                    source: SourceSpan::default(),
                }],
                source: SourceSpan::default(),
            });
        }

        None
    }
}

fn flush_text_piece<Ext>(pieces: &mut Vec<InlinePiece<Ext>>, text: &mut String) {
    if text.is_empty() {
        return;
    }
    pieces.push(InlinePiece::Text(core::mem::take(text)));
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-requires-matching-delimiters]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-emphasis-requires-matching-delimiters]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-permitted]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-is-not-allowed-inside-words]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-permitted]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-forbidden]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-consecutive-emphasis]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-produces-nested-strong]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.both-multiples-of-three-allow-emphasis-match]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-allows-indefinite-nesting]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-emphasis-allows-indefinite-nesting]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-emphasis]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-strong]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-can-nest-inside-strong-with-same-delimiter]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-delimiters-must-match]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.emphasis-may-contain-any-nonempty-inlines]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.strong-emphasis-may-contain-any-nonempty-inlines]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.empty-asterisk-emphasis-is-not-allowed]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.empty-underscore-emphasis-is-not-allowed]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.is-emphasis-within-emphasis]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-11-puts-excess-asterisks-outside-emphasis]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-12-puts-excess-underscores-outside-emphasis]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-13-applies-to-long-delimiter-runs]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rule-13-requires-different-directly-nested-delimiters]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.rules-capture-patterns-without-backtracking]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.rules-illustrated-by-examples]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.less-common-but-useful-emphasis-patterns]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.ambiguity-resolution-principles]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.original-markdown-description]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.original-markdown-description-quoted]
// [spec:gfm:sem:inlines.emphasis-and-strong-emphasis.original-rules-leave-nesting-underdetermined]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.literal-asterisk-cannot-edge-asterisk-emphasis]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.literal-underscore-cannot-edge-underscore-delimiters]
// [spec:gfm:def:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.trickiest-inline-case]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-initial-current-position]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-loop]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-stack-bottom-limits-scan]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-tracks-openers-bottom]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-updates-openers-bottom]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-consumes-delimiters]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-chooses-node-kind]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-inserts-node]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-interior-delimiters]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-advances]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-inactive-closer]
// [spec:gfm:sem:appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-cleans-up-stack]
fn resolve_emphasis<Ext>(mut pieces: Vec<InlinePiece<Ext>>) -> Vec<Node<Ext>>
where
    Ext: Clone + PartialEq,
{
    loop {
        let mut changed = false;

        'outer: for closer_index in 0..pieces.len() {
            let (marker, closer_count, can_close, can_open, closer_byte_pos) =
                match &pieces[closer_index] {
                    InlinePiece::Delimiter {
                        marker,
                        count,
                        can_open,
                        can_close,
                        byte_pos,
                    } => (*marker, *count, *can_close, *can_open, *byte_pos),
                    _ => continue,
                };
            if !can_close || closer_count == 0 {
                continue;
            }

            for opener_index in (0..closer_index).rev() {
                let (open_marker, opener_count, opener_can_open, opener_can_close, opener_byte_pos) =
                    match &pieces[opener_index] {
                        InlinePiece::Delimiter {
                            marker,
                            count,
                            can_open,
                            can_close,
                            byte_pos,
                        } => (*marker, *count, *can_open, *can_close, *byte_pos),
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

                // Span covers the consumed delimiters on both sides:
                // opener consumes its last `used` chars; closer consumes its
                // first `used` chars.
                let span = SourceSpan::new(opener_byte_pos + opener_rem, closer_byte_pos + used);

                let children = materialize_inline_pieces(inner);
                let wrapped = if used == 2 {
                    Node::Strong {
                        children,
                        source: span,
                    }
                } else {
                    Node::Emphasis {
                        children,
                        source: span,
                    }
                };

                let mut replacement = Vec::new();
                if opener_rem > 0 {
                    replacement.push(InlinePiece::Delimiter {
                        marker,
                        count: opener_rem,
                        can_open: opener_can_open,
                        can_close: opener_can_close,
                        byte_pos: opener_byte_pos,
                    });
                }
                replacement.push(InlinePiece::Node(wrapped));
                if closer_rem > 0 {
                    replacement.push(InlinePiece::Delimiter {
                        marker,
                        count: closer_rem,
                        can_open,
                        can_close,
                        byte_pos: closer_byte_pos + used,
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

    let nodes = materialize_inline_pieces(pieces);
    collapse_nested_emphasis(nodes)
}

fn collapse_nested_emphasis<Ext>(nodes: Vec<Node<Ext>>) -> Vec<Node<Ext>>
where
    Ext: Clone + PartialEq,
{
    nodes
        .into_iter()
        .map(|node| match node {
            Node::Strong { children, source } => {
                let children = flatten_same_type_children(collapse_nested_emphasis(children), true);
                Node::Strong { children, source }
            }
            Node::Emphasis { children, source } => {
                let children = collapse_nested_emphasis(children);
                Node::Emphasis { children, source }
            }
            other => other,
        })
        .collect()
}

fn flatten_same_type_children<Ext>(children: Vec<Node<Ext>>, is_strong: bool) -> Vec<Node<Ext>> {
    let mut out = Vec::with_capacity(children.len());
    for child in children {
        let is_same = if is_strong {
            matches!(&child, Node::Strong { .. })
        } else {
            matches!(&child, Node::Emphasis { .. })
        };
        if is_same {
            let inner = match child {
                Node::Strong { children, .. } | Node::Emphasis { children, .. } => children,
                _ => unreachable!(),
            };
            out.extend(inner);
        } else {
            out.push(child);
        }
    }
    out
}

fn materialize_inline_pieces<Ext>(pieces: Vec<InlinePiece<Ext>>) -> Vec<Node<Ext>> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            InlinePiece::Text(text) if !text.is_empty() => push_or_merge_text(&mut out, text),
            InlinePiece::Node(node) => out.push(node),
            InlinePiece::Delimiter { marker, count, .. } if count > 0 => {
                let ch = match marker {
                    EmphasisMarker::Asterisk => '*',
                    EmphasisMarker::Underscore => '_',
                };
                push_or_merge_text(&mut out, ch.to_string().repeat(count));
            }
            _ => {}
        }
    }
    out
}

fn push_or_merge_text<Ext>(out: &mut Vec<Node<Ext>>, text: String) {
    if let Some(Node::Text { text: existing, .. }) = out.last_mut() {
        existing.push_str(&text);
    } else {
        out.push(Node::Text {
            text,
            source: SourceSpan::default(),
        });
    }
}

fn char_before(input: &str, index: usize) -> Option<char> {
    input[..index].chars().next_back()
}

fn char_after(input: &str, index: usize) -> Option<char> {
    input.get(index..)?.chars().next()
}

// [spec:gfm:def:preliminaries.characters-and-lines.unicode-whitespace-character-definition]
// [spec:gfm:def:preliminaries.characters-and-lines.unicode-whitespace-definition]
// [spec:gfm:def:preliminaries.characters-and-lines.whitespace-character-definition]
// [spec:gfm:def:preliminaries.characters-and-lines.whitespace-definition]
// [spec:gfm:def:preliminaries.characters-and-lines.non-whitespace-character-definition]
// [spec:gfm:def:preliminaries.characters-and-lines.space-definition]
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

// [spec:gfm:def:preliminaries.characters-and-lines.punctuation-character-definition]
// [spec:gfm:def:preliminaries.characters-and-lines.ascii-punctuation-character-definition]
fn is_unicode_punctuation(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch.is_ascii_punctuation() || ch.is_ascii_graphic() && !ch.is_alphanumeric())
}

// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-condition-explanation]
// [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-alternative]
fn multiple_of_three_blocked(
    opener_count: usize,
    closer_count: usize,
    opener_can_close: bool,
    closer_can_open: bool,
) -> bool {
    let one_can_both = opener_can_close || closer_can_open;
    one_can_both
        && (opener_count + closer_count).is_multiple_of(3)
        && !(opener_count.is_multiple_of(3) && closer_count.is_multiple_of(3))
}

// [spec:gfm:sem:inlines.code-spans.all-space-content-is-not-trimmed]
// [spec:gfm:sem:inlines.code-spans.only-ascii-space-is-trimmed]
fn normalize_code_span(code: &str) -> String {
    if code.starts_with(' ')
        && code.ends_with(' ')
        && code.len() > 1
        && !code.chars().all(|ch| ch == ' ')
    {
        code[1..code.len() - 1].to_owned()
    } else {
        code.to_owned()
    }
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
    let search_start = index + run;
    let rest = input.get(search_start..)?;
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let j = i + bytes[i..].iter().take_while(|&&b| b == b'`').count();
            if j - i == run {
                return Some(search_start + i + run);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

fn find_delimited_closer(input: &str, mut index: usize, delim: &str) -> Option<usize> {
    while index < input.len() {
        if let Some(end) = code_span_end_at(input, index) {
            index = end;
            continue;
        }

        if input[index..].starts_with(delim) {
            // For tilde strikethrough, the closer must be a run of EXACTLY
            // delim.len() tildes (not part of a longer run).
            let is_tilde = delim == "~~" || delim == "~";
            let exact_run_ok = if is_tilde {
                let run = input[index..].chars().take_while(|c| *c == '~').count();
                run == delim.len()
            } else {
                true
            };
            if exact_run_ok && (!is_tilde || is_valid_tilde_closer(input, index)) {
                return Some(index);
            }
            // Skip past this whole tilde run if it's the wrong length
            if is_tilde {
                let run = input[index..].chars().take_while(|c| *c == '~').count();
                index += run;
            } else {
                index += delim.len();
            }
            continue;
        }

        let ch = input[index..].chars().next()?;
        index += ch.len_utf8();
    }

    None
}

// [spec:gfm:def:inlines.links.link-label-definition]
// [spec:gfm:syn:inlines.links.link-label-ends-at-first-unescaped-closing-bracket]
// [spec:gfm:syn:inlines.links.link-label-cannot-contain-unescaped-brackets]
// [spec:gfm:syn:inlines.links.link-label-length-is-at-most-999-characters]
// [spec:gfm:syn:inlines.links.link-label-must-contain-at-least-one-non-whitespace-character]
// [spec:gfm:def:inlines.links.link-label-must-contain-non-whitespace]
// [spec:gfm:sem:inlines.links.link-labels-cannot-contain-unescaped-brackets]
fn find_closing_bracket(input: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    let mut i = start;
    while i < input.len() {
        if escaped {
            escaped = false;
            i += input[i..].chars().next()?.len_utf8();
            continue;
        }
        let ch = input[i..].chars().next()?;
        match ch {
            '\\' => {
                escaped = true;
                i += 1;
            }
            '`' => {
                if let Some(end) = code_span_end_at(input, i) {
                    i = end;
                } else {
                    i += 1;
                }
            }
            '<' => {
                // HTML tags and autolinks bind tighter than link brackets
                if let Some(close) = input[i..].find('>') {
                    i += close + 1;
                } else {
                    i += 1;
                }
            }
            '[' => {
                depth += 1;
                i += 1;
            }
            ']' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
                i += 1;
            }
            _ => i += ch.len_utf8(),
        }
    }
    None
}

// [spec:gfm:syn:inlines.links.link-destination-definition]
// [spec:gfm:syn:inlines.links.destination-allows-normal-backslash-escaping]
// [spec:gfm:syn:inlines.links.destination-cannot-contain-line-breaks]
// [spec:gfm:syn:inlines.links.destination-may-contain-right-parenthesis-in-angle-brackets]
// [spec:gfm:syn:inlines.links.destination-may-contain-spaces-only-in-angle-brackets]
// [spec:gfm:syn:inlines.links.destination-may-include-fragments-and-queries]
// [spec:gfm:syn:inlines.links.destination-preserves-url-escaping-and-allows-entity-references]
// [spec:gfm:syn:inlines.links.balanced-parentheses-are-allowed-in-destination]
// [spec:gfm:def:inlines.links.implementations-may-limit-parenthesis-nesting-in-destinations]
// [spec:gfm:syn:inlines.links.parentheses-in-destination-may-be-escaped]
// [spec:gfm:syn:inlines.links.unbalanced-parentheses-require-escaping-or-angle-brackets]
// [spec:gfm:syn:inlines.links.unmatched-angle-brackets-do-not-form-destination]
// [spec:gfm:syn:inlines.links.angle-brackets-around-destination-must-be-unescaped]
// [spec:gfm:syn:inlines.links.backslash-before-non-escapable-character-is-literal-in-destination]
// [spec:gfm:syn:inlines.links.trailing-bracket-is-not-escaped-by-backslash-before-backslash]
// [spec:gfm:syn:inlines.links.destination-and-title-may-both-be-omitted]
fn parse_inline_link_target(rest: &str) -> Option<(usize, String, Option<String>)> {
    let trimmed = rest.trim_start();

    if let Some(after_angle) = trimmed.strip_prefix('<') {
        let angle_close = find_angle_close(after_angle)?;
        let destination = process_escapes_and_entities(&after_angle[..angle_close]);
        let after_gt = &after_angle[angle_close + 1..];
        let paren_in_after_gt = find_closing_paren(after_gt)?;
        let title_body = after_gt[..paren_in_after_gt].trim();
        let title = if title_body.is_empty() {
            None
        } else {
            // Non-empty content after > must be a valid title, otherwise reject
            Some(process_escapes_and_entities(&parse_title(title_body)?))
        };
        let paren_pos_in_rest = rest.len() - after_gt.len() + paren_in_after_gt;
        return Some((paren_pos_in_rest + 1, destination, title));
    }

    let closing = find_closing_paren(rest)?;
    let body = rest[..closing].trim();

    if body.is_empty() {
        return Some((closing + 1, String::new(), None));
    }

    if let Some((destination, title)) = split_destination_and_title(body) {
        if destination.contains(|ch: char| ch.is_ascii_whitespace() || ch == '<' || ch == '>') {
            return None;
        }
        return Some((
            closing + 1,
            process_escapes_and_entities(&destination),
            title.map(|t| process_escapes_and_entities(&t)),
        ));
    }

    if body.contains(|ch: char| ch.is_ascii_whitespace() || ch == '<' || ch == '>') {
        return None;
    }
    Some((closing + 1, process_escapes_and_entities(body), None))
}

fn find_closing_paren(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn find_angle_close(input: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '>' => return Some(i),
            '\n' => return None,
            '<' => return None,
            _ => {}
        }
    }
    None
}

fn process_escapes_and_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();
    while i < input.len() {
        if bytes[i] == b'\\' && i + 1 < input.len() {
            let next = input[i + 1..].chars().next().unwrap();
            if is_escapable(next) {
                out.push(next);
                i += 1 + next.len_utf8();
                continue;
            }
        }
        if bytes[i] == b'&'
            && let Some((consumed, decoded)) = parse_entity_reference(&input[i..])
        {
            out.push_str(&decoded);
            i += consumed;
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn parse_title(s: &str) -> Option<String> {
    let first = s.chars().next()?;
    let close = match first {
        '"' => '"',
        '\'' => '\'',
        '(' => ')',
        _ => return None,
    };
    if s.len() < 2 {
        return None;
    }
    // Find the first unescaped closing delimiter.
    let body = &s[first.len_utf8()..];
    let mut escaped = false;
    for (i, ch) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == close {
            let after_close = &body[i + ch.len_utf8()..];
            if after_close.is_empty() {
                return Some(body[..i].to_owned());
            }
            // There is text after the closing delimiter -- invalid title.
            return None;
        }
        // For parenthesized titles, nested open-parens are not allowed.
        if first == '(' && ch == '(' {
            return None;
        }
    }
    None
}

// [spec:gfm:req:inlines.links.link-title-definition]
// [spec:gfm:syn:inlines.links.titles-may-use-single-quotes-double-quotes-or-parentheses]
// [spec:gfm:syn:inlines.links.titles-allow-backslash-escapes-and-entity-references]
// [spec:gfm:def:inlines.links.link-titles-may-span-lines-but-not-blank-lines]
// [spec:gfm:syn:inlines.links.nested-balanced-quotes-in-titles-require-escaping]
// [spec:gfm:syn:inlines.links.different-title-delimiter-can-avoid-escaping]
// [spec:gfm:req:inlines.links.title-cannot-appear-without-destination]
// [spec:gfm:syn:inlines.links.title-may-be-omitted]
// [spec:gfm:syn:inlines.links.title-must-be-separated-from-destination-by-whitespace]
fn split_destination_and_title(body: &str) -> Option<(String, Option<String>)> {
    let body_trimmed = body.trim();
    for (i, ch) in body_trimmed.char_indices() {
        if !ch.is_ascii_whitespace() {
            continue;
        }
        let after = body_trimmed[i..].trim_start();
        if after.is_empty() {
            continue;
        }
        let marker = after.chars().next().unwrap();
        if !matches!(marker, '"' | '\'' | '(') {
            continue;
        }
        let destination = body_trimmed[..i].trim();
        if destination.is_empty() {
            continue;
        }
        let title = parse_title(after);
        if let Some(title) = title {
            return Some((destination.to_owned(), Some(title)));
        }
    }
    None
}

// [spec:gfm:syn:leaf-blocks.html-blocks.non-type-6-tag-must-be-complete-and-alone-on-first-line]
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

// [spec:gfm:syn:leaf-blocks.html-blocks.opening-tag-indent-up-to-three-spaces]
fn trimmed_html_block_line(line: &str) -> Option<&str> {
    let indent = leading_indent(line);
    (indent.columns <= 3).then_some(&line[indent.bytes..])
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-1-pre-example]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-1-script-example]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-1-style-example]
fn starts_html_type_1(line: &str) -> bool {
    HTML_BLOCK_TYPE_1_TAGS
        .iter()
        .any(|tag| matches_html_tag_prefix(line, tag))
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-2-comment-example]
fn starts_html_type_2(line: &str) -> bool {
    line.starts_with("<!--")
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-3-processing-instruction-example]
fn starts_html_type_3(line: &str) -> bool {
    line.starts_with("<?")
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-4-declaration-example]
fn starts_html_type_4(line: &str) -> bool {
    line.starts_with("<!")
        && !line.starts_with("<!--")
        && !line.starts_with("<![CDATA[")
        && line
            .chars()
            .nth(2)
            .is_some_and(|ch| ch.is_ascii_uppercase())
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-5-cdata-example]
fn starts_html_type_5(line: &str) -> bool {
    line.starts_with("<![CDATA[")
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-examples]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-initial-tag-need-not-be-alone-on-line]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-initial-tag-need-not-be-valid]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-may-start-with-closing-tag]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-open-tag-need-not-be-closed]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-opening-tag-may-be-partial-when-split-at-whitespace]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-6-partial-tag-need-not-be-completed]
fn starts_html_type_6(line: &str) -> bool {
    let Some(name) = parse_html_block_tag_start_name(line) else {
        return false;
    };
    is_html_block_tag_name(name, HTML_BLOCK_TYPE_6_TAGS)
}

// [spec:gfm:syn:leaf-blocks.html-blocks.type-7-example-cannot-interrupt-paragraph]
// [spec:gfm:syn:leaf-blocks.html-blocks.type-7-tag-name-may-be-anything]
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

// [spec:gfm:syn:inlines.raw-html.tag-name-definition]
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

// [spec:gfm:def:inlines.raw-html.attribute-name-definition]
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

// [spec:gfm:def:inlines.raw-html.unquoted-attribute-value-definition]
// [spec:gfm:def:inlines.raw-html.single-quoted-attribute-value-definition]
// [spec:gfm:def:inlines.raw-html.double-quoted-attribute-value-definition]
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

// [spec:gfm:def:inlines.raw-html.attribute-definition]
// [spec:gfm:def:inlines.raw-html.attribute-value-specification-definition]
// [spec:gfm:def:inlines.raw-html.open-tag-definition]
fn parse_html_open_tag_prefix(line: &str) -> Option<(&str, usize)> {
    let rest = line.strip_prefix('<')?;
    if rest.starts_with('/') {
        return None;
    }

    let (name, name_len) = parse_html_tag_name_prefix(rest)?;
    let mut consumed = 1 + name_len;
    let mut saw_whitespace = false;

    loop {
        let rest = &line[consumed..];
        if rest.starts_with('>') {
            return Some((name, consumed + 1));
        }
        if rest.starts_with("/>") {
            return Some((name, consumed + 2));
        }

        let whitespace = count_html_whitespace(rest);
        if whitespace > 0 {
            consumed += whitespace;
            saw_whitespace = true;
            continue;
        }

        // Non-whitespace, non-`>` — must be an attribute name, which requires
        // at least one whitespace character before it.
        if !saw_whitespace {
            return None;
        }
        saw_whitespace = false;

        consumed += parse_html_attribute_name_prefix(&line[consumed..])?;

        // Skip optional whitespace around `=`
        let before_eq = consumed + count_html_whitespace(&line[consumed..]);
        if line[before_eq..].starts_with('=') {
            consumed = before_eq + 1;
            consumed += count_html_whitespace(&line[consumed..]);
            consumed += parse_html_attribute_value_prefix(&line[consumed..])?;
        }
    }
}

// [spec:gfm:def:inlines.raw-html.closing-tag-definition]
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

#[allow(clippy::too_many_arguments)]
fn parse_inline_children<Ext>(
    text: &str,
    extensions: &[Box<dyn Extension<Ext>>],
    refs: &ReferenceDefinitions,
    footnote_labels: &FootnoteLabels,
    provisional: bool,
    gfm: bool,
    math: bool,
    highlights: bool,
) -> Result<Vec<Node<Ext>>, Error>
where
    Ext: Clone + PartialEq,
{
    let mut parser = InlineParser {
        input: text,
        index: 0,
        base_offset: 0,
        extensions,
        refs,
        footnote_labels,
        provisional,
        gfm,
        math,
        highlights,
    };
    parser.parse_until(None)
}

// [spec:gfm:syn:inlines.images.html-rendering-typically-uses-plain-description-as-alt]
fn contains_link<Ext>(nodes: &[Node<Ext>]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Link { .. } => true,
        Node::Emphasis { children, .. }
        | Node::Strong { children, .. }
        | Node::Strikethrough { children, .. } => contains_link(children),
        _ => false,
    })
}

fn build_link_or_image<Ext>(
    image: bool,
    destination: String,
    title: Option<String>,
    children: Vec<Node<Ext>>,
    source: SourceSpan,
) -> Node<Ext> {
    if image {
        Node::Image {
            destination,
            title,
            alt: plain_text(&children),
            source,
        }
    } else {
        Node::Link {
            destination,
            title,
            children,
            source,
        }
    }
}

fn build_item_node<Ext>(children: Vec<ParsedBlock<Ext>>) -> Node<Ext>
where
{
    let mut iter = children.into_iter();
    match iter.next() {
        Some(ParsedBlock {
            node:
                Node::TaskItem {
                    checked,
                    children: task_children,
                    source,
                },
            ..
        }) => {
            // Append any subsequent siblings as additional children of the task item.
            let mut all = task_children;
            all.extend(iter.map(|child| child.node));
            Node::TaskItem {
                checked,
                children: all,
                source,
            }
        }
        first => {
            let mut all = Vec::new();
            if let Some(first) = first {
                all.push(first.node);
            }
            all.extend(iter.map(|child| child.node));
            Node::Item {
                children: all,
                source: SourceSpan::default(),
            }
        }
    }
}

// [spec:gfm:def:container-blocks.task-list-items-extension.definition]
// [spec:gfm:def:container-blocks.task-list-items-extension.marker-structure]
// [spec:gfm:sem:container-blocks.task-list-items-extension.whitespace-marker-is-unchecked]
// [spec:gfm:req:container-blocks.task-list-items-extension.checkbox-interaction-is-unspecified]
// [spec:gfm:sem:container-blocks.task-list-items-extension.gfm-enables-task-list-extension]
// [spec:gfm:syn:container-blocks.task-list-items-extension.task-lists-may-be-arbitrarily-nested]
fn normalize_task_item<Ext>(mut children: Vec<ParsedBlock<Ext>>) -> Vec<ParsedBlock<Ext>>
where
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
            source: SourceSpan::default(),
        }],
        source: SourceSpan::default(),
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

fn is_tight_list<Ext>(items: &[ParsedListItem<Ext>]) -> bool {
    if items.iter().any(|item| item.loose) {
        return false;
    }
    // A trailing blank on any item (except the last) means blank lines between items → loose
    for (i, item) in items.iter().enumerate() {
        if i < items.len() - 1 && item.trailing_blank {
            return false;
        }
    }
    true
}

fn is_paragraph_continuation_candidate(line: &str) -> bool {
    // Returns true if the given line COULD be an open paragraph that accepts
    // lazy continuation. Excludes blank lines and lines that start non-paragraph
    // block constructs (code, thematic breaks, headings, HTML blocks, etc.).
    if line.trim().is_empty() {
        return false;
    }
    if parse_fence_opener(line).is_some() {
        return false;
    }
    if is_thematic_break(line) {
        return false;
    }
    if strip_indented_code_line(line).is_some() {
        return false;
    }
    if classify_html_block_start(line).is_some() {
        return false;
    }
    let trimmed = trim_up_to_three_spaces(line);
    if trimmed.starts_with('#') {
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        let after = &trimmed[hashes..];
        if (1..=6).contains(&hashes) && (after.is_empty() || after.starts_with([' ', '\t'])) {
            return false;
        }
    }
    true
}

fn is_lazy_list_continuation(line: &str, lines: &[String], index: usize, gfm: bool) -> bool {
    !line.trim().is_empty()
        && parse_list_marker(line).is_none()
        && strip_block_quote_marker(line).is_none()
        && strip_indented_code_line(line).is_none()
        && parse_fence_opener(line).is_none()
        && !is_thematic_break(line)
        && parse_reference_definition(lines, index).is_none()
        && !(gfm && parse_table_delimiter(line).is_some())
        && !classify_html_block_start(line).is_some_and(|start| start.can_interrupt_paragraph)
        && !matches!(trim_up_to_three_spaces(line).chars().next(), Some('#'))
        && !(gfm
            && line.contains('|')
            && index + 1 < lines.len()
            && parse_table_delimiter(&lines[index + 1]).is_some())
}

/// Like `is_lazy_list_continuation` but for block quotes: indented code does
/// NOT disqualify, because indented code cannot interrupt a paragraph.
fn is_lazy_block_quote_continuation(line: &str, lines: &[String], index: usize, gfm: bool) -> bool {
    !line.trim().is_empty()
        && parse_list_marker(line).is_none_or(|m| !can_interrupt_paragraph_with_list(m))
        && strip_block_quote_marker(line).is_none()
        && parse_fence_opener(line).is_none()
        && !is_thematic_break(line)
        && parse_reference_definition(lines, index).is_none()
        && !(gfm && parse_table_delimiter(line).is_some())
        && !classify_html_block_start(line).is_some_and(|start| start.can_interrupt_paragraph)
        && !matches!(trim_up_to_three_spaces(line).chars().next(), Some('#'))
        && !(gfm
            && line.contains('|')
            && index + 1 < lines.len()
            && parse_table_delimiter(&lines[index + 1]).is_some())
}

fn plain_text<Ext>(nodes: &[Node<Ext>]) -> String {
    let mut out = String::new();
    for node in nodes {
        node.walk(&mut |n| match n {
            Node::Text { text, .. } => out.push_str(text),
            Node::CodeSpan { code, .. } => out.push_str(code),
            Node::SoftBreak { .. } | Node::HardBreak { .. } => out.push(' '),
            Node::FootnoteRef { label, .. } => out.push_str(label),
            Node::Image { alt, .. } => out.push_str(alt),
            Node::CodeBlock { literal, .. } => out.push_str(literal),
            Node::MathInline { math, .. } | Node::MathBlock { math, .. } => out.push_str(math),
            Node::Html { html, .. } => out.push_str(html),
            _ => {}
        });
    }
    out
}

// [spec:gfm:sem:leaf-blocks.link-reference-definitions.label-matching-is-case-insensitive]
// [spec:gfm:req:inlines.links.label-match-definition]
// [spec:gfm:sem:inlines.links.label-matching-collapses-consecutive-internal-whitespace]
// [spec:gfm:syn:inlines.links.label-matching-uses-unicode-case-fold]
fn normalize_reference_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// [spec:gfm:sem:inlines.entity-and-numeric-character-references.references-produce-unicode-characters]
// [spec:gfm:def:inlines.entity-and-numeric-character-references.decimal-numeric-reference-definition]
// [spec:gfm:def:inlines.entity-and-numeric-character-references.hex-reference-definition]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.invalid-references-are-literal-text]
// [spec:gfm:def:inlines.entity-and-numeric-character-references.entity-references-definition]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.named-entities-require-semicolon]
// [spec:gfm:req:inlines.entity-and-numeric-character-references.parsers-need-not-preserve-source-form]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-are-literal-text-in-code]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-are-not-recognized-in-code]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-are-recognized-outside-code]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-cannot-replace-structural-markers]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.references-cannot-replace-structure-symbols]
// [spec:gfm:syn:inlines.entity-and-numeric-character-references.unknown-named-entities-are-literal]
fn parse_entity_reference(input: &str) -> Option<(usize, String)> {
    if !input.starts_with('&') {
        return None;
    }
    let end = input.find(';')?;
    let entity = &input[1..end];
    if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        let ch = u32::from_str_radix(hex, 16).ok().and_then(|n| {
            if n == 0 {
                Some('\u{FFFD}')
            } else {
                char::from_u32(n)
            }
        })?;
        Some((end + 1, ch.to_string()))
    } else if let Some(num) = entity.strip_prefix('#') {
        let ch = num.parse::<u32>().ok().and_then(|n| {
            if n == 0 {
                Some('\u{FFFD}')
            } else {
                char::from_u32(n)
            }
        })?;
        Some((end + 1, ch.to_string()))
    } else {
        // [spec:gfm:def:inlines.entity-and-numeric-character-references.entity-references-definition]
        let decoded = resolve_named_entity(entity)?;
        Some((end + 1, decoded.to_string()))
    }
}

fn resolve_named_entity(name: &str) -> Option<&'static str> {
    use entities::ENTITIES;
    let lookup = format!("&{name};");
    ENTITIES
        .iter()
        .find(|e| e.entity == lookup)
        .map(|e| e.characters)
}

// [spec:gfm:def:inlines.autolinks.absolute-uri-definition]
// [spec:gfm:def:inlines.autolinks.scheme-definition]
// [spec:gfm:syn:inlines.autolinks.autolink-schemes-are-case-insensitive]
fn looks_like_url(text: &str) -> bool {
    let Some(colon_pos) = text.find(':') else {
        return false;
    };
    let scheme = &text[..colon_pos];
    if scheme.is_empty() || scheme.len() > 32 {
        return false;
    }
    let mut chars = scheme.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() {
        return false;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '.' | '-')) {
        return false;
    }
    if scheme.len() < 2 {
        return false;
    }
    let after_scheme = &text[colon_pos + 1..];
    !after_scheme
        .chars()
        .any(|ch| ch.is_whitespace() || ch == '<' || ch == '>')
}

// [spec:gfm:def:inlines.autolinks-extension.trailing-punctuation-is-excluded-from-autolinks]
// [spec:gfm:syn:inlines.autolinks-extension.trailing-semicolon-is-excluded-when-it-looks-like-entity]
// [spec:gfm:syn:inlines.autolinks-extension.unmatched-trailing-right-parentheses-are-excluded]
// [spec:gfm:syn:inlines.autolinks-extension.trailing-parenthesis-check-applies-only-at-link-end]
// [spec:gfm:syn:inlines.autolinks-extension.left-angle-bracket-ends-autolink]
/// GFM extended www autolinks: the host portion of the URL must consist of
/// dot-separated DNS labels, and the LAST TWO labels of the host must not
/// contain underscores.
fn is_valid_www_host(url: &str) -> bool {
    // Extract host: everything before the first '/', '?', '#'.
    let host_end = url.find(['/', '?', '#']).unwrap_or(url.len());
    let host = &url[..host_end];
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n < 2 {
        return false;
    }
    // Check the last two labels for underscores
    if labels[n - 1].contains('_') || labels[n - 2].contains('_') {
        return false;
    }
    true
}

fn trim_autolink_trailing(url: &str) -> &str {
    let mut end = url.len();
    let bytes = url.as_bytes();

    loop {
        if end == 0 {
            break;
        }
        let last = bytes[end - 1];

        if matches!(
            last,
            b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' | b'\'' | b'"'
        ) {
            end -= 1;
            continue;
        }

        if last == b')' {
            let open = url[..end].matches('(').count();
            let close = url[..end].matches(')').count();
            if close > open {
                end -= 1;
                continue;
            }
        }

        if last == b';'
            && let Some(amp) = url[..end].rfind('&')
        {
            let entity = &url[amp + 1..end - 1];
            if !entity.is_empty() && entity.chars().all(|c| c.is_ascii_alphanumeric()) {
                // Strip the entire &...; entity-like sequence
                end = amp;
                continue;
            }
        }

        break;
    }

    &url[..end]
}

const DISALLOWED_HTML_TAGS: &[&str] = &[
    "textarea",
    "title",
    "style",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "script",
    "plaintext",
];

// [spec:gfm:def:inlines.disallowed-raw-html-extension.disallowed-tags]
// [spec:gfm:sem:inlines.disallowed-raw-html-extension.filtering-replaces-leading-angle-bracket]
// [spec:gfm:def:inlines.disallowed-raw-html-extension.gfm-enables-tagfilter-extension]
// [spec:gfm:sem:inlines.disallowed-raw-html-extension.all-other-html-tags-are-left-untouched]
// [spec:gfm:sem:inlines.disallowed-raw-html-extension.disallowed-tags-change-html-parsing]
pub(crate) fn filter_disallowed_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut i = 0;
    let bytes = html.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let after_angle = i + 1;
            let is_closing = after_angle < bytes.len() && bytes[after_angle] == b'/';
            let tag_name_start = if is_closing {
                after_angle + 1
            } else {
                after_angle
            };

            let mut matched = false;
            for tag in DISALLOWED_HTML_TAGS {
                let tag_end = tag_name_start + tag.len();
                if tag_end <= bytes.len() {
                    let candidate = &html[tag_name_start..tag_end];
                    if candidate.eq_ignore_ascii_case(tag) {
                        let boundary_ok = tag_end >= bytes.len()
                            || matches!(bytes[tag_end], b' ' | b'\t' | b'\n' | b'>' | b'/');
                        if boundary_ok {
                            result.push_str("&lt;");
                            i += 1;
                            matched = true;
                            break;
                        }
                    }
                }
            }

            if !matched {
                result.push('<');
                i += 1;
            }
        } else {
            result.push(html[i..].chars().next().unwrap());
            i += html[i..].chars().next().unwrap().len_utf8();
        }
    }

    result
}

// [spec:gfm:def:inlines.autolinks-extension.extended-email-autolink-rules]
// [spec:gfm:syn:inlines.autolinks-extension.extended-email-autolinks-insert-mailto-scheme]
// [spec:gfm:syn:inlines.autolinks-extension.dot-dash-and-underscore-rules-around-at-sign]
// [spec:gfm:sem:inlines.autolinks-extension.plus-is-allowed-before-but-not-after-at-sign]
fn looks_like_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    if local.is_empty()
        || !local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return false;
    }
    if local.ends_with('-') {
        return false;
    }
    if domain.is_empty()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.ends_with('-')
    {
        return false;
    }
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
    {
        return false;
    }
    true
}

// [spec:gfm:def:preliminaries.characters-and-lines.character-class-definitions]
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
    use crate::error::{Error, StreamError};
    use crate::ext::{BlockMatch, Extension, InlineMatch};
    use crate::node::{Node, SourceSpan, StreamItem};
    use crate::sink::VecSink;

    // [spec:adv:req:markdown.input.byte-cap/test]
    #[test]
    fn default_parser_has_no_input_cap() {
        let mut parser = Parser::<()>::new();
        let mut sink = VecSink::<Node<()>>::new();
        // 256 KB benign input — no cap, should succeed.
        let huge = "a\n\n".repeat(85_000);
        parser.push_str(&huge, &mut sink).expect("no cap set");
        parser.finish(&mut sink).expect("finish");
    }

    // [spec:adv:req:markdown.input.byte-cap/test]
    // [spec:mw:def:core.error.stream-error/test]
    #[test]
    fn single_push_over_cap_rejected() {
        let mut parser = Parser::<()>::builder().with_max_input_bytes(1024).build();
        let mut sink = VecSink::<Node<()>>::new();
        let huge = "a".repeat(2048);
        let err = parser.push_str(&huge, &mut sink).unwrap_err();
        match err {
            StreamError::Parser(Error::InputTooLarge { seen, cap }) => {
                assert_eq!(seen, 2048);
                assert_eq!(cap, 1024);
            }
            other => panic!("expected InputTooLarge, got {other:?}"),
        }
    }

    // [spec:adv:req:markdown.input.byte-cap/test]
    #[test]
    fn streaming_push_cumulative_trips_cap() {
        let mut parser = Parser::<()>::builder().with_max_input_bytes(10).build();
        let mut sink = VecSink::<Node<()>>::new();
        // Three 4-byte chunks = 12 bytes total — trips at the third.
        parser.push_str("aaaa", &mut sink).expect("first chunk");
        parser.push_str("bbbb", &mut sink).expect("second chunk");
        let err = parser.push_str("cccc", &mut sink).unwrap_err();
        assert!(matches!(
            err,
            StreamError::Parser(Error::InputTooLarge { seen: 12, cap: 10 })
        ));
    }

    // [spec:adv:req:markdown.input.byte-cap/test]
    #[test]
    fn exactly_at_cap_ok() {
        let mut parser = Parser::<()>::builder().with_max_input_bytes(8).build();
        let mut sink = VecSink::<Node<()>>::new();
        parser.push_str("aaaabbbb", &mut sink).expect("at cap ok");
        parser.finish(&mut sink).expect("finish ok");
    }

    // [spec:adv:req:markdown.input.byte-cap/test]
    #[test]
    fn usize_max_effectively_unlimited() {
        let mut parser = Parser::<()>::builder()
            .with_max_input_bytes(usize::MAX)
            .build();
        let mut sink = VecSink::<Node<()>>::new();
        parser
            .push_str(&"a".repeat(100_000), &mut sink)
            .expect("ok");
        parser.finish(&mut sink).expect("finish");
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestExt {
        Math(String),
        Note(String),
    }

    struct MathExtension;

    impl Extension<TestExt> for MathExtension {
        fn try_inline(&self, input: &str) -> Option<InlineMatch<TestExt>> {
            let rest = input.strip_prefix('$')?;
            let end = rest.find('$')?;
            Some(InlineMatch {
                consumed: end + 2,
                node: Node::Custom {
                    payload: TestExt::Math(rest[..end].to_owned()),
                    children: Vec::new(),
                    source: SourceSpan::default(),
                },
            })
        }
    }

    /// Inline extension that wraps `!![text]!!` runs as a TestExt::Note
    /// node whose children come from recursively parsing `text` via the
    /// provided `InlineContext`. Used to verify extensions can run the
    /// full inline parser on captured substrings (emphasis, other
    /// extensions, etc.).
    struct NoteInlineExtension;

    impl Extension<TestExt> for NoteInlineExtension {
        fn try_inline_ctx(
            &self,
            input: &str,
            ctx: &dyn crate::ext::InlineContext<TestExt>,
        ) -> Option<InlineMatch<TestExt>> {
            let rest = input.strip_prefix("!![")?;
            let end = rest.find("]!!")?;
            let inner = &rest[..end];
            let children = ctx.parse_inline(inner);
            Some(InlineMatch {
                consumed: 3 + inner.len() + 3,
                node: Node::Custom {
                    payload: TestExt::Note(inner.to_owned()),
                    children,
                    source: SourceSpan::default(),
                },
            })
        }
    }

    /// A block extension that matches `:::note` / `:::` fences and captures
    /// the enclosed body as a single Note payload.
    struct NoteBlockExtension;

    impl Extension<TestExt> for NoteBlockExtension {
        fn try_block(&self, lines: &[String], start: usize) -> Option<BlockMatch<TestExt>> {
            if lines[start].trim() != ":::note" {
                return None;
            }
            let mut body = Vec::new();
            let mut index = start + 1;
            while index < lines.len() {
                if lines[index].trim() == ":::" {
                    return Some(BlockMatch {
                        consumed_lines: index - start + 1,
                        node: Node::Custom {
                            payload: TestExt::Note(body.join("\n")),
                            children: Vec::new(),
                            source: SourceSpan::default(),
                        },
                    });
                }
                body.push(lines[index].clone());
                index += 1;
            }
            None
        }
    }

    // [spec:mw:sem:core.stream.provisional/test]
    // [spec:mw:def:core.sink.vec-sink/test]
    #[test]
    fn emits_stable_ids_for_provisional_emphasis_upgrade() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:leaf-blocks.setext-headings.structure/test]
    #[test]
    fn upgrades_setext_heading() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:leaf-blocks.fenced-code-blocks.content-continues-until-matching-closing-fence/test]
    #[test]
    fn supports_fenced_code_blocks() {
        let mut parser = Parser::<()>::new();
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
                if info.as_deref() == Some("rust") && literal == "fn main() {}\n"
        ));
    }

    #[test]
    fn supports_block_quotes() {
        let mut parser = Parser::<()>::new();
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
            .next_back()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::BlockQuote { children, .. }
                if matches!(children.as_slice(), [Node::Heading { level, .. }, Node::Paragraph { .. }] if *level == 1)
        ));
    }

    #[test]
    fn supports_simple_lists() {
        let mut parser = Parser::<()>::new();
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
            .next_back()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::List { ordered: false, children, .. }
                if matches!(children.as_slice(), [Node::Item { .. }, Node::Item { .. }])
        ));
    }

    #[test]
    fn supports_indented_code_blocks() {
        let mut parser = Parser::<()>::new();
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
            .next_back()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::CodeBlock { info: None, literal, .. } if literal == "foo\nbar\n"
        ));
    }

    // [spec:mw:def:core.node.custom-payload/test]
    // [spec:mw:def:core.ext.trait/test]
    // [spec:mw:req:core.ext.dispatch-order/test]
    // [spec:mw:req:core.ext.ctx-delegation/test]
    #[test]
    fn supports_inline_extension_hooks() {
        let mut parser = Parser::builder().with_extension(MathExtension).build();
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

    // [spec:mw:def:core.ext.inline-context/test]
    #[test]
    fn supports_recursive_inline_parse_from_extension() {
        // NoteInlineExtension captures `!![...]!!` and recursively parses
        // the inner text via the InlineContext. Verifies that the full
        // inline parser runs on the captured substring — in particular,
        // `*x*` inside the extension's capture becomes a Node::Emphasis
        // rather than literal text. This proves extensions can run
        // recursive inline parses without a nested Parser hack.
        let mut parser = Parser::builder()
            .with_extension(NoteInlineExtension)
            .build();
        let mut sink = VecSink::new();
        parser
            .push_str("prefix !![before *italic* after]!! tail", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();
        let nodes: Vec<_> = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(e) if !e.provisional => Some(e.node.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(nodes.len(), 1);
        let Node::Paragraph { children, .. } = &nodes[0] else {
            panic!("expected paragraph, got {:?}", nodes[0]);
        };
        assert!(matches!(&children[0], Node::Text { text, .. } if text == "prefix "));
        let Node::Custom {
            children: note_children,
            ..
        } = &children[1]
        else {
            panic!("expected Note custom, got {:?}", children[1]);
        };
        let has_emphasis = note_children.iter().any(|n| {
            matches!(
                n,
                Node::Emphasis { children, .. }
                    if matches!(children.as_slice(), [Node::Text { text, .. }] if text == "italic")
            )
        });
        assert!(
            has_emphasis,
            "expected emphasis from recursive parse, got: {note_children:?}"
        );
    }

    // [spec:mw:def:core.ext.match-types/test]
    #[test]
    fn supports_block_extension_hooks() {
        let mut parser = Parser::builder().with_extension(NoteBlockExtension).build();
        let mut sink = VecSink::new();

        parser
            .push_str("before\n\n:::note\nhello\nworld\n:::\n\nafter\n", &mut sink)
            .unwrap();
        parser.finish(&mut sink).unwrap();

        let nodes: Vec<_> = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(e) if !e.provisional => Some(e.node.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(nodes.len(), 3);
        assert!(matches!(&nodes[0], Node::Paragraph { .. }));
        match &nodes[1] {
            Node::Custom { payload, .. } => {
                assert_eq!(payload, &TestExt::Note("hello\nworld".to_owned()));
            }
            other => panic!("expected custom note block, got {other:?}"),
        }
        assert!(matches!(&nodes[2], Node::Paragraph { .. }));
    }

    // [spec:gfm:syn:inlines.links.inline-link-definition/test]
    // [spec:gfm:syn:inlines.images.definition/test]
    #[test]
    fn supports_inline_links_and_images() {
        let mut parser = Parser::<()>::new();
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
            .next_back()
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

    // [spec:gfm:sem:leaf-blocks.link-reference-definitions.may-appear-before-or-after-uses/test]
    // [spec:gfm:syn:inlines.links.shortcut-reference-link-definition/test]
    #[test]
    fn resolves_reference_links_from_later_definitions() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:def:inlines.raw-html.html-like-tag-text-is-parsed-as-raw-html/test]
    #[test]
    fn supports_inline_raw_html() {
        let mut parser = Parser::<()>::new();
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
            .next_back()
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

    // [spec:gfm:def:leaf-blocks.html-blocks.definition/test]
    #[test]
    fn supports_html_blocks() {
        let mut parser = Parser::<()>::new();
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
            .next_back()
            .unwrap();

        assert!(matches!(
            final_node,
            Node::Html { html, .. } if html == "<div>\nfoo\n</div>"
        ));
    }

    // [spec:gfm:syn:leaf-blocks.html-blocks.opening-tag-indent-up-to-three-spaces/test]
    #[test]
    fn html_blocks_only_start_with_up_to_three_spaces_of_indent() {
        let mut parser = Parser::<()>::new();
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
                if first == "   <div>" && literal == "<div>\n"
        ));
    }

    // [spec:gfm:syn:leaf-blocks.html-blocks.type-7-example-cannot-interrupt-paragraph/test]
    #[test]
    fn type_7_html_blocks_do_not_interrupt_paragraphs() {
        let mut parser = Parser::<()>::new();
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
            .next_back()
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

    // [spec:gfm:def:inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters/test]
    // [spec:gfm:syn:inlines.autolinks.definition/test]
    // [spec:gfm:syn:inlines.autolinks-extension.extended-url-autolink-definition/test]
    #[test]
    fn supports_strikethrough_and_autolinks() {
        let mut parser = Parser::<()>::builder().with_gfm().build();
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
            .next_back()
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

    // [spec:gfm:def:container-blocks.task-list-items-extension.definition/test]
    // [spec:gfm:def:leaf-blocks.tables-extension.table-definition/test]
    // [spec:mw:def:core.node.table-shape/test]
    #[test]
    fn supports_task_list_items_and_tables() {
        let mut parser = Parser::<()>::builder().with_gfm().build();
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

    // [spec:gfm:syn:container-blocks.list-items.lazy-continuation-example/test]
    // [spec:gfm:def:container-blocks.lists.loose-list-definition/test]
    #[test]
    fn detects_loose_lists_and_lazy_continuation() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:container-blocks.list-items.ordered-start-number-is-at-most-nine-digits/test]
    // [spec:gfm:syn:container-blocks.list-items.ordered-start-number-may-have-leading-zeroes/test]
    #[test]
    fn enforces_ordered_list_marker_width_rules() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:container-blocks.list-items.interrupting-ordered-list-must-start-at-one/test]
    #[test]
    fn ordered_lists_only_interrupt_paragraphs_at_one() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:container-blocks.list-items.empty-item-does-not-require-post-marker-spaces/test]
    #[test]
    fn supports_empty_list_items_without_post_marker_space() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:container-blocks.list-items.interrupting-first-item-cannot-begin-with-blank-line/test]
    // [spec:gfm:syn:container-blocks.list-items.blank-line-start-makes-post-marker-spacing-irrelevant/test]
    #[test]
    fn blank_start_list_item_does_not_interrupt_paragraph_and_uses_normalized_indent() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:inlines.emphasis-and-strong-emphasis.both-multiples-of-three-allow-emphasis-match/test]
    #[test]
    fn preserves_chunk_invariant_for_nested_delimiters() {
        let input = "foo***bar***baz\n";

        let mut one = Parser::<()>::new();
        let mut one_sink = VecSink::new();
        one.push_str(input, &mut one_sink).unwrap();
        one.finish(&mut one_sink).unwrap();

        let mut chunked = Parser::<()>::new();
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

    // [spec:gfm:thm:container-blocks.list-items.sublists-follow-general-indentation-rules/test]
    #[test]
    fn supports_nested_sublists() {
        let mut parser = Parser::<()>::new();
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

    // [spec:gfm:syn:container-blocks.list-items.indented-code-must-be-four-spaces-beyond-content-indent/test]
    #[test]
    fn supports_indented_code_inside_list_items() {
        let mut parser = Parser::<()>::new();
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
                    if matches!(children.as_slice(), [Node::Paragraph { .. }, Node::CodeBlock { literal, .. }] if literal == "bar\n"))
        ));
    }

    // [spec:gfm:sem:preliminaries.tabs.tabs-not-expanded-in-lines/test]
    #[test]
    fn preserves_tabs_in_non_structural_text() {
        let mut parser = Parser::<()>::new();
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

    // [spec:mw:req:core.stream.batch-ordering/test]
    #[test]
    fn deletes_blocks_that_turn_into_reference_definitions() {
        let mut parser = Parser::<()>::new();
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

        let mut one = Parser::<()>::new();
        let mut one_sink = VecSink::new();
        one.push_str(input, &mut one_sink).unwrap();
        one.finish(&mut one_sink).unwrap();

        let mut chunked = Parser::<()>::new();
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
    fn defaults_to_pure_commonmark() {
        let mut parser = Parser::<()>::new();
        let mut sink = VecSink::new();

        // GFM table syntax → stays as paragraphs (pipe-separated text)
        parser
            .push_str("| a | b |\n| - | - |\n| 1 | 2 |\n\n", &mut sink)
            .unwrap();
        // GFM strikethrough → literal tildes
        parser.push_str("~~strike~~\n\n", &mut sink).unwrap();
        // Footnote ref → literal text
        parser.push_str("[^ref]\n\n", &mut sink).unwrap();
        // Math → literal dollar signs
        parser.push_str("$x^2$\n\n", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        let nodes: Vec<_> = sink
            .items()
            .iter()
            .filter_map(|item| match item {
                StreamItem::Upsert(e) if !e.provisional => Some(&e.node),
                _ => None,
            })
            .collect();

        // All should be paragraphs — no Table, Strikethrough, FootnoteRef, MathInline
        for node in &nodes {
            assert!(
                matches!(node, Node::Paragraph { .. }),
                "expected Paragraph, got {node:?}"
            );
        }
    }

    // [spec:mw:def:core.error.taxonomy/test]
    #[test]
    fn rejects_new_input_after_finish() {
        let mut parser = Parser::<()>::new();
        let mut sink = VecSink::new();

        parser.push_str("hello", &mut sink).unwrap();
        parser.finish(&mut sink).unwrap();

        assert!(parser.push_str(" world", &mut sink).is_err());
    }
}
