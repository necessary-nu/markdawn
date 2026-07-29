//! Block-level half of the ANSI adapter: one writer per `Node` kind,
//! dispatched from [`AnsiAdapter::write_block`]. Inline rendering, the
//! adapter's types and the column/wrapping helpers live in the parent
//! `ansi` module.

use super::{AnsiAdapter, AnsiError, AnsiWalker, RenderAnsiCustom, visible_len, write_wrapped};
use crate::ansi_style::Color;
use crate::node::{AlertKind, Node, TableAlignment};

impl<R> AnsiAdapter<R> {
    pub(super) fn write_block<Ext>(
        &self,
        node: &Node<Ext>,
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        match node {
            Node::Document { children, .. } => self.write_children(children, out)?,
            Node::Paragraph { children, .. } => self.write_paragraph(children, out)?,
            Node::Heading {
                level, children, ..
            } => self.write_heading(*level, children, out)?,
            Node::BlockQuote { children, .. } => self.write_block_quote(children, out)?,
            Node::List {
                ordered,
                start,
                children,
                ..
            } => self.write_list(*ordered, *start, children, out)?,
            // Reachable if callers hand us a bare Item. The List handler
            // normally consumes these.
            Node::Item { children, .. } | Node::TaskItem { children, .. } => {
                self.write_children(children, out)?
            }
            Node::Table {
                alignments,
                header,
                rows,
                ..
            } => self.write_table(alignments, header, rows, out)?,
            Node::TableRow { .. } | Node::TableCell { .. } => {
                // Handled via the parent Table node only.
            }
            Node::CodeBlock { info, literal, .. } => {
                self.write_code_block(info.as_deref(), literal, out)
            }
            Node::MathBlock { math, .. } => {
                out.push_str(math);
                out.push_str("\n\n");
            }
            Node::ThematicBreak { .. } => self.write_thematic_break(out),
            Node::Alert {
                kind,
                title,
                children,
                ..
            } => self.write_alert(kind, title.as_deref(), children, out)?,
            Node::DefinitionList { children, .. } => self.write_definition_list(children, out)?,
            Node::DefinitionTerm { .. } | Node::DefinitionDescription { .. } => {
                // Handled by the DefinitionList parent only.
            }
            Node::Html { .. } => {
                // Drop HTML — no reasonable rendering in a terminal.
            }
            Node::FootnoteSection { children, .. } => self.write_footnote_section(children, out)?,
            Node::FootnoteDefinition {
                label, children, ..
            } => self.write_footnote_definition(label, children, out)?,
            Node::Custom {
                payload, children, ..
            } => self.write_custom(payload, children, out)?,
            // Leaves at block level pass through as inlines (e.g. a bare
            // Text at document root).
            Node::Text { .. }
            | Node::CodeSpan { .. }
            | Node::Emphasis { .. }
            | Node::Strong { .. }
            | Node::Strikethrough { .. }
            | Node::Subscript { .. }
            | Node::Superscript { .. }
            | Node::Highlight { .. }
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::MathInline { .. }
            | Node::SoftBreak { .. }
            | Node::HardBreak { .. }
            | Node::FootnoteRef { .. } => {
                self.write_inline(node, out)?;
            }
        }
        Ok(())
    }

    fn write_children<Ext>(&self, children: &[Node<Ext>], out: &mut String) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        for child in children {
            self.write_block(child, out)?;
        }
        Ok(())
    }

    /// Render `children` to their own buffer so the block can be measured
    /// or line-prefixed before it reaches `out`.
    fn block_buffer<Ext>(&self, children: &[Node<Ext>]) -> Result<String, AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let mut buf = String::new();
        for child in children {
            self.write_block(child, &mut buf)?;
        }
        Ok(buf)
    }

    fn write_paragraph<Ext>(
        &self,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let mut buf = String::new();
        self.write_inlines(children, &mut buf)?;
        if let Some(w) = self.options.width {
            write_wrapped(&buf, 0, w, out);
        } else {
            out.push_str(&buf);
        }
        out.push_str("\n\n");
        Ok(())
    }

    fn write_heading<Ext>(
        &self,
        level: u8,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let accent = match level {
            1 => Some(Color::Named(6)),
            2 => Some(Color::Named(5)),
            _ => None,
        };
        let mut style = String::from("\x1b[1");
        if let Some(accent) = accent {
            style.push(';');
            style.push_str(&self.sgr_color(accent, false));
        }
        style.push('m');
        let mut text = String::new();
        self.write_inlines(children, &mut text)?;
        out.push_str(&style);
        out.push_str(&text);
        out.push_str("\x1b[0m\n");
        if level == 1 {
            let width = self
                .options
                .width
                .unwrap_or_else(|| visible_len(&text).max(1));
            let bar_width = visible_len(&text).min(width);
            out.push_str(&style);
            for _ in 0..bar_width {
                out.push('━');
            }
            out.push_str("\x1b[0m\n");
        }
        out.push('\n');
        Ok(())
    }

    fn write_block_quote<Ext>(
        &self,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let buf = self.block_buffer(children)?;
        for line in buf.split_inclusive('\n') {
            if line == "\n" {
                out.push_str("\x1b[2m│\x1b[0m\n");
            } else {
                out.push_str("\x1b[2m│\x1b[0m ");
                out.push_str(line);
            }
        }
        Ok(())
    }

    fn write_list<Ext>(
        &self,
        ordered: bool,
        start: Option<u64>,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let mut count = start.unwrap_or(1);
        for child in children {
            let prefix = if ordered {
                let p = format!("  {count}. ");
                count += 1;
                p
            } else {
                "  • ".to_string()
            };
            let body = self.list_item_body(child)?;
            let mut first = true;
            for line in body.split_inclusive('\n') {
                if first {
                    out.push_str(&prefix);
                    first = false;
                } else if line != "\n" {
                    for _ in 0..visible_len(&prefix) {
                        out.push(' ');
                    }
                }
                out.push_str(line);
            }
        }
        out.push('\n');
        Ok(())
    }

    /// An item's blocks rendered without its marker; a non-item child is
    /// rendered as the block it is.
    fn list_item_body<Ext>(&self, child: &Node<Ext>) -> Result<String, AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        match child {
            Node::Item { children, .. } | Node::TaskItem { children, .. } => {
                self.block_buffer(children)
            }
            other => {
                let mut body = String::new();
                self.write_block(other, &mut body)?;
                Ok(body)
            }
        }
    }

    // Three-way dispatch on the first word of the info string. All three
    // forms guarantee a trailing newline plus a blank separator line.
    // [spec:mw:req:ansi.adapter.code-block]
    fn write_code_block(&self, info: Option<&str>, literal: &str, out: &mut String) {
        let lang = info
            .and_then(|s| s.split_whitespace().next())
            .filter(|s| !s.is_empty());
        if matches!(lang, Some("ansi")) {
            write_styled_literal(literal, out);
        } else if matches!(lang, Some("terminal")) {
            self.write_terminal_transcript(literal, out);
        } else {
            self.write_fenced_literal(lang, literal, out);
        }
    }

    /// Run the literal through the VT100 emulator and re-emit the final
    /// visible screen as styled ANSI. Drops the literal cursor-traffic
    /// bytes entirely.
    fn write_terminal_transcript(&self, literal: &str, out: &mut String) {
        #[cfg(feature = "terminal")]
        {
            let mut grid = crate::terminal::ansi_snapshot_grid(literal, None, None);
            // The transcript's own colours are the only truecolour the
            // adapter ever re-encodes, so this is where a depth downgrade
            // actually bites.
            // [spec:mw:req:ansi.adapter.color-depth-inert+1]
            for row in &mut grid.cells {
                for cell in row.iter_mut() {
                    cell.style.fg = cell
                        .style
                        .fg
                        .map(|c| super::downgrade(c, self.options.color));
                    cell.style.bg = cell
                        .style
                        .bg
                        .map(|c| super::downgrade(c, self.options.color));
                }
            }
            out.push_str(&crate::terminal::render_grid_ansi(&grid));
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        #[cfg(not(feature = "terminal"))]
        {
            out.push_str(literal);
            if !literal.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
    }

    /// A plain fenced block: dim language tag, then every line on a banded
    /// background.
    fn write_fenced_literal(&self, lang: Option<&str>, literal: &str, out: &mut String) {
        if let Some(lang) = lang {
            out.push_str("\x1b[2m");
            out.push_str(lang);
            out.push_str("\x1b[0m\n");
        }
        let band = format!("\x1b[{}m", self.sgr_color(Color::Palette(236), true));
        for line in literal.split_inclusive('\n') {
            out.push_str(&band);
            out.push_str("  ");
            let needs_nl = line.ends_with('\n');
            let body = if needs_nl {
                &line[..line.len() - 1]
            } else {
                line
            };
            out.push_str(body);
            out.push_str("\x1b[0m");
            if needs_nl {
                out.push('\n');
            }
        }
        if !literal.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }

    fn write_thematic_break(&self, out: &mut String) {
        let width = self.options.width.unwrap_or(60);
        out.push_str("\x1b[2m");
        for _ in 0..width {
            out.push('─');
        }
        out.push_str("\x1b[0m\n\n");
    }

    fn write_alert<Ext>(
        &self,
        kind: &AlertKind,
        title: Option<&str>,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let (accent, default_title) = match kind {
            AlertKind::Note => (Color::Named(4), "Note"),
            AlertKind::Tip => (Color::Named(2), "Tip"),
            AlertKind::Important => (Color::Named(3), "Important"),
            AlertKind::Warning => (Color::Named(1), "Warning"),
            AlertKind::Caution => (Color::Named(9), "Caution"),
        };
        let color = self.sgr_color(accent, false);
        out.push_str("\x1b[1;");
        out.push_str(&color);
        out.push('m');
        out.push_str(title.unwrap_or(default_title));
        out.push_str("\x1b[0m\n");
        let body = self.block_buffer(children)?;
        for line in body.split_inclusive('\n') {
            out.push_str("\x1b[");
            out.push_str(&color);
            if line == "\n" {
                out.push_str("m│\x1b[0m\n");
            } else {
                out.push_str("m│\x1b[0m ");
                out.push_str(line);
            }
        }
        Ok(())
    }

    fn write_definition_list<Ext>(
        &self,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        for chunk in children.chunks(2) {
            if let [
                Node::DefinitionTerm { children: term, .. },
                Node::DefinitionDescription { children: desc, .. },
            ] = chunk
            {
                out.push_str("\x1b[1m");
                self.write_inlines(term, out)?;
                out.push_str("\x1b[0m\n");
                self.write_definition_body(desc, out)?;
            }
        }
        out.push('\n');
        Ok(())
    }

    fn write_definition_body<Ext>(
        &self,
        desc: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        for d in desc {
            let mut body = String::new();
            self.write_block(d, &mut body)?;
            for line in body.split_inclusive('\n') {
                if line != "\n" {
                    out.push_str("  ");
                }
                out.push_str(line);
            }
        }
        Ok(())
    }

    fn write_footnote_section<Ext>(
        &self,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        if children.is_empty() {
            return Ok(());
        }
        out.push_str("\x1b[2m---\x1b[0m\n");
        self.write_children(children, out)
    }

    fn write_footnote_definition<Ext>(
        &self,
        label: &str,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        out.push_str("\x1b[2m[^");
        out.push_str(label);
        out.push_str("]\x1b[0m ");
        self.write_children(children, out)
    }

    /// Hand a custom payload to the configured renderer, giving it closures
    /// to recurse back into block and inline rendering.
    pub(super) fn write_custom<Ext>(
        &self,
        payload: &Ext,
        children: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let walk_block = |node: &Node<Ext>, o: &mut String| self.write_block(node, o);
        let walk_inlines = |nodes: &[Node<Ext>], o: &mut String| self.write_inlines(nodes, o);
        let walker = AnsiWalker {
            walk_block: &walk_block,
            walk_inlines: &walk_inlines,
        };
        self.custom.render_custom(payload, children, out, &walker)
    }

    fn write_table<Ext>(
        &self,
        alignments: &[Option<TableAlignment>],
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
        out: &mut String,
    ) -> Result<(), AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        // Very small table renderer — one-pass rendering of cell text,
        // column widths from visible length, ASCII box-drawing. No
        // multi-line cells.
        let cells = self.table_cell_text(header, rows)?;
        if cells.is_empty() {
            return Ok(());
        }
        let columns = cells.iter().map(|c| c.len()).max().unwrap_or(1);
        let mut widths = vec![0usize; columns];
        for row in &cells {
            for (i, text) in row.iter().enumerate() {
                widths[i] = widths[i].max(visible_len(text));
            }
        }

        let draw_border = |out: &mut String, left: char, mid: char, right: char, fill: char| {
            out.push(left);
            for (i, w) in widths.iter().enumerate() {
                for _ in 0..w + 2 {
                    out.push(fill);
                }
                if i + 1 < widths.len() {
                    out.push(mid);
                }
            }
            out.push(right);
            out.push('\n');
        };
        let draw_row = |out: &mut String, line: &[String]| {
            out.push('│');
            for (i, w) in widths.iter().enumerate() {
                out.push(' ');
                let cell = line.get(i).map(|s| s.as_str()).unwrap_or("");
                out.push_str(cell);
                let pad = w.saturating_sub(visible_len(cell));
                let align = alignments.get(i).copied().flatten();
                match align {
                    Some(TableAlignment::Right) => {}
                    _ => {
                        for _ in 0..pad {
                            out.push(' ');
                        }
                    }
                }
                out.push(' ');
                out.push('│');
            }
            out.push('\n');
        };

        draw_border(out, '┌', '┬', '┐', '─');
        if !header.is_empty() {
            draw_row(out, &cells[0]);
            draw_border(out, '├', '┼', '┤', '─');
            for line in &cells[1..] {
                draw_row(out, line);
            }
        } else {
            for line in &cells {
                draw_row(out, line);
            }
        }
        draw_border(out, '└', '┴', '┘', '─');
        out.push('\n');
        Ok(())
    }

    /// Header rows followed by body rows, each row rendered to one string
    /// of styled cell text.
    fn table_cell_text<Ext>(
        &self,
        header: &[Node<Ext>],
        rows: &[Node<Ext>],
    ) -> Result<Vec<Vec<String>>, AnsiError>
    where
        R: RenderAnsiCustom<Ext>,
    {
        let mut cells: Vec<Vec<String>> = Vec::new();
        for row in header.iter().chain(rows.iter()) {
            if let Node::TableRow {
                cells: row_cells, ..
            } = row
            {
                let mut line = Vec::with_capacity(row_cells.len());
                for cell in row_cells {
                    if let Node::TableCell { children, .. } = cell {
                        let mut text = String::new();
                        self.write_inlines(children, &mut text)?;
                        line.push(text);
                    }
                }
                cells.push(line);
            }
        }
        Ok(cells)
    }
}

/// Content that is already styled — emit verbatim and guarantee a reset
/// plus the blank separator line follow.
fn write_styled_literal(literal: &str, out: &mut String) {
    out.push_str(literal);
    if !literal.ends_with("\x1b[0m") {
        out.push_str("\x1b[0m");
    }
    if !literal.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}
