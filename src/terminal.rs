//! VT100-class terminal emulation on top of `vt100`.
//!
//! Where [`crate::ansi_to_html`] is a byte-stream → HTML translator that
//! only understands SGR + OSC 8 (and silently drops cursor movement,
//! clears, scroll regions, alt-screen swaps, etc.), this module runs the
//! input through a real terminal emulator and returns a snapshot of the
//! final *visible* screen. Same HTML span/color shape on the output
//! side — the input path is the difference.
//!
//! Use this for embedding real terminal sessions (e.g. an interactive
//! `cargo build` with progress ticks, anything that touches alt-screen)
//! where the literal bytestream doesn't match what a human would see.
//! For pre-styled static output (a colored tree dump, a rustc error
//! message), stick with `ansi_to_html` — it's cheaper and doesn't
//! impose a fixed grid width.
//!
//! Emulation is **stateful and whole-buffer** — you feed bytes, then
//! snapshot. There is no streaming equivalent; the `AnsiSink` streaming
//! surface is orthogonal and stays in the `ansi` pipeline.

use crate::ansi_style::{CellStyle, Color, color_to_sgr};

/// Default terminal width when the caller doesn't specify one. Matches
/// the historical 80-column DEC VT100 default.
pub const DEFAULT_COLS: u16 = 80;

/// Upper bound on auto-grown row count for snapshot rendering. Prevents
/// a runaway transcript from blowing up the output page.
pub const MAX_AUTO_ROWS: u16 = 200;

/// One rendered cell: the Unicode character the terminal would draw at
/// this position plus the style it would draw it with. Wide characters
/// claim two consecutive cells (the second cell has an empty string).
// `ch` is a `String`, not a `char`, because a cell may hold a whole
// grapheme cluster. `is_blank` is the predicate both renderers trim
// trailing run-out with.
// [spec:mw:def:ansi.term.grid]
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cell {
    pub ch: String,
    pub style: CellStyle,
}

impl Cell {
    pub fn is_blank(&self) -> bool {
        self.style.is_plain() && (self.ch.is_empty() || self.ch == " ")
    }
}

/// Owned snapshot of the visible terminal after feeding bytes. This is
/// the stable output shape — callers render it however they like.
// [spec:mw:def:ansi.term.grid]
#[derive(Clone, Debug)]
pub struct TermGrid {
    pub rows: u16,
    pub cols: u16,
    pub cells: Vec<Vec<Cell>>,
    pub cursor: (u16, u16),
    pub title: Option<String>,
    pub alternate_screen: bool,
}

impl TermGrid {
    /// True when every cell is blank.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|row| row.iter().all(Cell::is_blank))
    }
}

/// Stateful emulator. Feed bytes any number of times, then snapshot.
// A real emulator sits underneath, so cursor movement, erases, scroll
// regions, wrapping at `cols` and alt-screen swaps take effect on the
// grid rather than being dropped: the snapshot is what a human sees,
// not the literal bytestream. No streaming equivalent exists.
// [spec:mw:def:ansi.term.emulator]
pub struct TermEmulator {
    parser: vt100::Parser,
}

impl TermEmulator {
    /// Construct an emulator of the given grid size. `scrollback` rows
    /// are retained above the visible region (most callers pass 0 for
    /// "snapshot only the visible screen").
    pub fn with_size(rows: u16, cols: u16, scrollback: usize) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, scrollback),
        }
    }

    /// Default 24×80 grid, no scrollback.
    pub fn new() -> Self {
        Self::with_size(24, DEFAULT_COLS, 0)
    }

    /// Feed a chunk of bytes through the VT state machine.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Feed a str chunk.
    pub fn feed_str(&mut self, s: &str) {
        self.feed(s.as_bytes());
    }

    /// Take an owned snapshot of the current screen.
    pub fn snapshot(&self) -> TermGrid {
        grid_from_screen(self.parser.screen())
    }
}

impl Default for TermEmulator {
    fn default() -> Self {
        Self::new()
    }
}

fn grid_from_screen(screen: &vt100::Screen) -> TermGrid {
    let (rows, cols) = screen.size();
    let mut cells: Vec<Vec<Cell>> = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut line: Vec<Cell> = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            let cell = screen
                .cell(row, col)
                .map(cell_from_vt100)
                .unwrap_or_default();
            line.push(cell);
        }
        cells.push(line);
    }
    TermGrid {
        rows,
        cols,
        cells,
        cursor: screen.cursor_position(),
        // `vt100::Screen` 0.16 doesn't expose OSC 0/2 titles directly;
        // leave the field for future wiring via the callbacks API.
        title: None,
        alternate_screen: screen.alternate_screen(),
    }
}

// Intensity is a single exclusive slot in the emulator, so a cell is
// bold or dim but never both, and SGR 22 clears whichever is live.
// Strike has no representation at all: vt100 0.16 carries no mode bit
// for SGR 9 and offers no accessor, so the attribute is lost inside the
// emulator before a snapshot can see it.
// [spec:mw:req:ansi.term.style-mapping+1]
fn cell_from_vt100(cell: &vt100::Cell) -> Cell {
    let ch = if cell.has_contents() {
        cell.contents().to_string()
    } else {
        String::new()
    };
    let style = CellStyle {
        fg: vt100_color(cell.fgcolor()),
        bg: vt100_color(cell.bgcolor()),
        bold: cell.bold(),
        dim: cell.dim(),
        italic: cell.italic(),
        underline: cell.underline(),
        strike: false,
        inverse: cell.inverse(),
    };
    Cell { ch, style }
}

// The `n < 16` split is what lets `color_to_sgr` recover the short
// 30-37 / 90-97 forms when re-encoding.
// [spec:mw:req:ansi.term.style-mapping+1]
fn vt100_color(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(n) if n < 16 => Some(Color::Named(n)),
        vt100::Color::Idx(n) => Some(Color::Palette(n)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

/// Render a `TermGrid` as an HTML fragment: rows separated by `\n`,
/// with `<span style="…">` runs for styled cells. Trailing blanks on
/// each row are trimmed. Use the same `<pre>` wrapper the ANSI block
/// toggle markup already provides.
// Adjacent cells with equal styles coalesce into one span, so a styled
// run is never split cell-by-cell and a plain run stays bare. An empty
// `ch` renders as a space so a wide char's trailing half doesn't
// shorten the line.
// [spec:mw:req:ansi.term.html-runs]
pub fn render_grid_html(grid: &TermGrid) -> String {
    let mut out = String::with_capacity(grid.rows as usize * grid.cols as usize);
    for (row_idx, row) in grid.cells.iter().enumerate() {
        let last_non_blank = row.iter().rposition(|c| !c.is_blank());
        let end = last_non_blank.map_or(0, |i| i + 1);
        let mut open_span: Option<CellStyle> = None;
        for cell in &row[..end] {
            let style = cell.style;
            if open_span != Some(style) {
                if open_span.is_some() {
                    out.push_str("</span>");
                    open_span = None;
                }
                if !style.is_plain() {
                    out.push_str("<span style=\"");
                    out.push_str(&style.css());
                    out.push_str("\">");
                    open_span = Some(style);
                }
            }
            push_char(&mut out, &cell.ch);
        }
        if open_span.is_some() {
            out.push_str("</span>");
        }
        if row_idx + 1 < grid.cells.len() {
            out.push('\n');
        }
    }
    out
}

/// Render a `TermGrid` as plain ANSI text (SGR + OSC 0 title only).
/// Newlines between rows; trailing blanks trimmed.
// Reset-then-set, not differential: `ESC[0m` precedes every style
// change (including a change *into* plain) and closes a row that ended
// non-plain, so no attribute can leak past its cell range.
// [spec:mw:sem:ansi.term.ansi-encoding]
pub fn render_grid_ansi(grid: &TermGrid) -> String {
    let mut out = String::new();
    for (row_idx, row) in grid.cells.iter().enumerate() {
        let last_non_blank = row.iter().rposition(|c| !c.is_blank());
        let end = last_non_blank.map_or(0, |i| i + 1);
        let mut current = CellStyle::default();
        for cell in &row[..end] {
            if cell.style != current {
                out.push_str("\x1b[0m");
                if !cell.style.is_plain() {
                    out.push_str(&style_to_sgr(&cell.style));
                }
                current = cell.style;
            }
            if cell.ch.is_empty() {
                out.push(' ');
            } else {
                out.push_str(&cell.ch);
            }
        }
        if !current.is_plain() {
            out.push_str("\x1b[0m");
        }
        if row_idx + 1 < grid.cells.len() {
            out.push('\n');
        }
    }
    out
}

// [spec:mw:sem:ansi.term.ansi-encoding]
fn style_to_sgr(style: &CellStyle) -> String {
    let mut parts: Vec<String> = Vec::new();
    if style.bold {
        parts.push("1".into());
    }
    if style.dim {
        parts.push("2".into());
    }
    if style.italic {
        parts.push("3".into());
    }
    if style.underline {
        parts.push("4".into());
    }
    if style.inverse {
        parts.push("7".into());
    }
    if style.strike {
        parts.push("9".into());
    }
    if let Some(c) = style.fg {
        parts.push(color_to_sgr(c, false));
    }
    if let Some(c) = style.bg {
        parts.push(color_to_sgr(c, true));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", parts.join(";"))
    }
}

fn push_char(out: &mut String, s: &str) {
    if s.is_empty() {
        out.push(' ');
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

/// One-shot convenience: feed `literal` through a fresh emulator and
/// return the snapshot as HTML.
///
/// Sizing: `cols` defaults to [`DEFAULT_COLS`] (80). `rows` defaults to
/// the maximum of the cursor's final row + 1 and the tallest non-blank
/// row, clamped at [`MAX_AUTO_ROWS`] (200).
pub fn ansi_snapshot_to_html(literal: &str, cols: Option<u16>, rows: Option<u16>) -> String {
    let grid = ansi_snapshot_grid(literal, cols, rows);
    render_grid_html(&grid)
}

/// One-shot convenience: same as [`ansi_snapshot_to_html`] but emits
/// ANSI-styled text.
pub fn ansi_snapshot_to_ansi(literal: &str, cols: Option<u16>, rows: Option<u16>) -> String {
    let grid = ansi_snapshot_grid(literal, cols, rows);
    render_grid_ansi(&grid)
}

/// Low-level snapshot constructor — returns the `TermGrid` so callers
/// can inspect cells / cursor directly. Internal but exposed for tests.
// `cols` is load-bearing — the emulator wraps on it — so it is clamped
// to at least 1. An explicit `rows` is honoured verbatim with no trim.
// The cursor row participates in the auto-trim maximum precisely so the
// trim never cuts above where the next write would land.
// [spec:mw:req:ansi.term.auto-size]
pub fn ansi_snapshot_grid(literal: &str, cols: Option<u16>, rows: Option<u16>) -> TermGrid {
    let cols = cols.unwrap_or(DEFAULT_COLS).max(1);
    // First pass: feed into a tall emulator to find out how many rows
    // the content actually needs, then clip down. 200 is the hard cap.
    let probe_rows = rows.unwrap_or(MAX_AUTO_ROWS).clamp(1, MAX_AUTO_ROWS);
    let mut emu = TermEmulator::with_size(probe_rows, cols, 0);
    emu.feed_str(literal);
    let grid = emu.snapshot();
    if rows.is_some() {
        return grid;
    }
    // Auto-trim trailing blank rows so the embed doesn't hog vertical
    // space when the program only painted the top few lines.
    let mut last_used = grid.cursor.0;
    for (idx, row) in grid.cells.iter().enumerate() {
        if row.iter().any(|c| !c.is_blank()) {
            last_used = last_used.max(idx as u16);
        }
    }
    let effective_rows = (last_used + 1).min(grid.rows);
    let trimmed_cells = grid
        .cells
        .into_iter()
        .take(effective_rows as usize)
        .collect();
    TermGrid {
        rows: effective_rows,
        cols: grid.cols,
        cells: trimmed_cells,
        cursor: grid.cursor,
        title: grid.title,
        alternate_screen: grid.alternate_screen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:def:ansi.term.emulator/test]
    #[test]
    fn erase_display_clears_prior_content() {
        // Print foo, then clear display + home cursor, then print X.
        // Snapshot MUST show only X at (0,0), not "foo".
        let html = ansi_snapshot_to_html("foo\x1b[2J\x1b[HX", Some(10), Some(3));
        assert!(
            !html.contains("foo"),
            "expected post-clear snapshot to drop 'foo'; got: {html}"
        );
        assert!(html.starts_with('X'), "expected X at (0,0); got: {html}");
    }

    #[test]
    fn cursor_move_overwrites_earlier_cells() {
        // abc, then cursor home, then X — should land on row 0 col 0,
        // leaving "Xbc" visible.
        let html = ansi_snapshot_to_html("abc\x1b[HX", Some(10), Some(2));
        assert!(
            html.contains("Xbc"),
            "expected Xbc after home+X; got: {html}"
        );
    }

    // [spec:mw:req:ansi.term.style-mapping+1/test]
    #[test]
    fn dim_survives_the_emulator_and_strike_does_not() {
        let dim = ansi_snapshot_grid("\x1b[2mfaint\x1b[0m", Some(20), Some(1));
        assert!(dim.cells[0][0].style.dim, "expected dim to reach the grid");
        assert!(
            ansi_snapshot_to_html("\x1b[2mfaint\x1b[0m", Some(20), Some(1)).contains("opacity:0.6")
        );

        // Intensity is one exclusive slot in the emulator, so the later
        // of bold / dim wins rather than both being live.
        let both = ansi_snapshot_grid("\x1b[1;2mx\x1b[0m", Some(20), Some(1));
        assert!(both.cells[0][0].style.dim);
        assert!(!both.cells[0][0].style.bold);

        // SGR 9 has no representation in the emulator at all.
        let struck = ansi_snapshot_grid("\x1b[9mgone\x1b[0m", Some(20), Some(1));
        assert!(!struck.cells[0][0].style.strike);
    }

    // [spec:mw:req:ansi.term.style-mapping+1/test]
    #[test]
    fn sgr_colors_roundtrip_in_snapshot() {
        let html = ansi_snapshot_to_html("\x1b[31mred\x1b[0m", Some(20), Some(2));
        assert!(
            html.contains("<span") && html.contains("red"),
            "expected styled span wrapping 'red'; got: {html}"
        );
    }

    // [spec:mw:req:ansi.term.html-runs/test]
    // [spec:mw:def:ansi.style.cell-style/test]
    #[test]
    fn plain_text_renders_without_spans() {
        let html = ansi_snapshot_to_html("hello", Some(20), Some(2));
        assert!(!html.contains("<span"));
        assert!(html.starts_with("hello"));
    }

    // [spec:mw:req:ansi.term.auto-size/test]
    // [spec:mw:def:ansi.term.grid/test]
    #[test]
    fn snapshot_auto_trims_trailing_blank_rows() {
        // Only one row of content — `rows=None` should auto-trim the
        // grid down to ~1 row, not 200.
        let grid = ansi_snapshot_grid("hi", Some(80), None);
        assert!(
            grid.rows <= 2,
            "auto-trim left too many rows: {}",
            grid.rows
        );
    }
}
