//! GFM table row scanning: delimiter-row alignment parsing and the
//! cell splitter that pairs each cell with the source range it came
//! from.

use crate::node::TableAlignment;

// [spec:gfm:def:leaf-blocks.tables-extension.delimiter-row-structure]
pub(super) fn parse_table_delimiter(line: &str) -> Option<Vec<Option<TableAlignment>>> {
    let cells = split_table_row(line);
    if cells.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(cells.len());
    for cell in cells {
        let trimmed = cell.trim();
        // A delimiter cell needs at least one `-` and may have leading/trailing `:`.
        if trimmed.is_empty()
            || !trimmed.chars().all(|ch| matches!(ch, '-' | ':'))
            || !trimmed.contains('-')
        {
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

// [spec:gfm:syn:leaf-blocks.tables-extension.escaped-pipes-are-literal-cell-content]
fn split_table_row(line: &str) -> Vec<String> {
    split_table_row_spans(line)
        .into_iter()
        .map(|(cell, _)| cell)
        .collect()
}

/// Split a table row into its cells, pairing each cell's content with the
/// byte range of the source text it was read from, relative to the start of
/// `line`. The content is trimmed and has `\|` escapes resolved, so it is not
/// in general the same string as the slice its range covers.
// [spec:mw:sem:core.span.unpopulated-is-empty+1]
pub(super) fn split_table_row_spans(line: &str) -> Vec<(String, core::ops::Range<usize>)> {
    let trimmed = line.trim();
    let leading = line.len() - line.trim_start().len();
    let (core, base) = match trimmed.strip_prefix('|') {
        Some(rest) => (rest, leading + 1),
        None => (trimmed, leading),
    };
    let core = core.strip_suffix('|').unwrap_or(core);

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut cell_start = 0;
    let mut chars = core.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '\\' && chars.peek().map(|(_, next)| *next) == Some('|') {
            chars.next();
            current.push('|');
            continue;
        }
        if ch == '|' {
            cells.push((
                core::mem::take(&mut current).trim().to_owned(),
                cell_range(core, cell_start..index, base),
            ));
            cell_start = index + 1;
        } else {
            current.push(ch);
        }
    }
    cells.push((
        current.trim().to_owned(),
        cell_range(core, cell_start..core.len(), base),
    ));
    cells
}

/// Byte range of the non-whitespace text of `core[range]`, shifted by `base`.
/// A cell holding only whitespace yields the empty range at its start.
// [spec:mw:req:core.span.non-inverted]
fn cell_range(core: &str, range: core::ops::Range<usize>, base: usize) -> core::ops::Range<usize> {
    let raw = &core[range.clone()];
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        let start = base + range.start;
        return start..start;
    }
    let start = base + range.start + (raw.len() - raw.trim_start().len());
    start..start + trimmed.len()
}

/// Byte range of the non-whitespace text of `line`, relative to its start.
// [spec:mw:req:core.span.non-inverted]
pub(super) fn trimmed_line_range(line: &str) -> core::ops::Range<usize> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return 0..0;
    }
    let start = line.len() - line.trim_start().len();
    start..start + trimmed.len()
}
