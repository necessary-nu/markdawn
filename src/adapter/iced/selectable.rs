//! Selectable rich-text widget for iced 0.14 with cross-block drag support.
//!
//! Selection state lives in a shared [`Selection`] handle (an
//! `Rc<RefCell<SelectionState>>`) created once by [`Renderer`] and
//! cloned into every [`SelectableRichText`]. Each widget reads the
//! shared anchor/cursor (in window coordinates) on every event/draw/
//! operate and clips against its own bounds — so a drag that starts in
//! one block and ends in another is naturally a single contiguous
//! selection.
//!
//! Architecture inspired by halloy's selectable_rich_text.rs (GPL-3.0)
//! but written from scratch — no copied code.

use std::any::Any;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use ::iced::Task;
use ::iced::advanced::layout::{self, Layout};
use ::iced::advanced::renderer;
use ::iced::advanced::svg;
use ::iced::advanced::text::{self, Paragraph as _};
use ::iced::advanced::widget::operation::{Operation, Outcome};
use ::iced::advanced::widget::{Widget, tree};
use ::iced::advanced::{Clipboard, Shell};
use ::iced::widget::text::{Catalog, LineHeight, Shaping, Wrapping};
use ::iced::{
    Color, Element, Event, Length, Pixels, Point, Rectangle, Size, Vector, alignment, mouse,
};

/// Minimum Manhattan distance the mouse must travel from its press
/// point before we promote a click into a drag-select. Below this
/// threshold the press is "still a click" — no anchor is set, no
/// highlight rect is painted. Defeats sub-pixel hand drift that would
/// otherwise produce one-character "ghost" highlights.
const DRAG_THRESHOLD_PX: f32 = 4.0;

/// Shared cross-block selection state. Window-local coordinates.
#[derive(Debug, Default, Clone, Copy)]
struct SelectionState {
    /// Press position waiting for the cursor to move at least
    /// `DRAG_THRESHOLD_PX` before becoming a real drag anchor.
    pending_anchor: Option<Point>,
    /// Real anchor — set only after the press→cursor distance has
    /// crossed `DRAG_THRESHOLD_PX`.
    anchor: Option<Point>,
    /// Current mouse position (or final position if `finalized`).
    cursor: Option<Point>,
    /// True after mouse-release — selection frozen until next press.
    finalized: bool,
}

/// Handle to cross-block selection state shared by every
/// [`SelectableRichText`] produced by the same [`super::Renderer`].
/// Cheap to clone (just an `Rc`).
#[derive(Debug, Default, Clone)]
pub struct Selection {
    inner: Rc<RefCell<SelectionState>>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mouse went down. Stash as a pending anchor — we won't promote it
    /// to a real drag until the cursor travels far enough.
    fn press(&self, point: Point) {
        let mut s = self.inner.borrow_mut();
        s.pending_anchor = Some(point);
        s.anchor = None;
        s.cursor = None;
        s.finalized = false;
    }

    /// Cursor moved. If pending and we've crossed the threshold,
    /// promote the pending anchor into a real drag. Otherwise just
    /// update the cursor position (for already-active drags).
    fn drag_to(&self, point: Point) {
        let mut s = self.inner.borrow_mut();
        if s.finalized {
            return;
        }
        if let Some(pending) = s.pending_anchor {
            let dist = (point.x - pending.x).abs() + (point.y - pending.y).abs();
            if dist >= DRAG_THRESHOLD_PX {
                // Promote.
                s.anchor = Some(pending);
                s.cursor = Some(point);
                s.pending_anchor = None;
            }
            // else: still a click-in-progress, stay quiet.
        } else if s.anchor.is_some() {
            s.cursor = Some(point);
        }
    }

    /// Mouse released. If a real drag is active, finalize it. Either
    /// way, clear the pending anchor (a click that didn't reach the
    /// threshold leaves no trace).
    fn release(&self) {
        let mut s = self.inner.borrow_mut();
        s.pending_anchor = None;
        if s.anchor.is_some() {
            s.finalized = true;
        }
    }

    fn clear(&self) {
        *self.inner.borrow_mut() = SelectionState::default();
    }

    #[cfg(test)]
    fn active(&self) -> bool {
        let s = self.inner.borrow();
        s.anchor.is_some() && s.cursor.is_some()
    }

    fn is_dragging(&self) -> bool {
        let s = self.inner.borrow();
        // Either we already promoted to a real drag, OR we have a
        // pending anchor — both states should keep the parent wrapper
        // calling `drag_to` so promotion can still happen.
        (s.anchor.is_some() || s.pending_anchor.is_some()) && !s.finalized
    }

    /// The selection range as `(top-left, bottom-right)` in window coords,
    /// or None if no selection is active.
    fn range(&self) -> Option<(Point, Point)> {
        let s = self.inner.borrow();
        let a = s.anchor?;
        let c = s.cursor?;
        let (start, end) = if (a.y, a.x) <= (c.y, c.x) {
            (a, c)
        } else {
            (c, a)
        };
        Some((start, end))
    }
}

/// A piece of inline content in a [`SelectableRichText`]. Either a
/// normal text span (which goes through cosmic-text's shaper as usual)
/// or a "replaced" atom — an inline image / SVG that occupies a fixed
/// width in the text flow but renders as an arbitrary widget. Modeled
/// after browsers' inline-block / Flutter's `WidgetSpan`.
pub enum InlineAtom<'a, F = ::iced::Font> {
    /// A regular text span — same as `text::Span` to `iced::widget::rich_text`.
    Span(text::Span<'a, String, F>),
    /// An inline replaced atom: rendered SVG inserted into the text
    /// flow at the position the source markdown placed it. The width
    /// is reserved during layout via a non-breaking placeholder; the
    /// SVG is overlaid during paint.
    Replaced {
        /// SVG bytes to paint at the atom's bounds.
        svg: Vec<u8>,
        /// Canonical source text put on the clipboard when the atom
        /// is in the user's selection (e.g. the LaTeX math source).
        copy_text: String,
        /// Intrinsic width of the SVG in em units (svg_pixel_width /
        /// font_size). Used to allocate the right amount of horizontal
        /// space in the line.
        em_width: f32,
        /// Intrinsic height of the SVG in em units (svg_pixel_height /
        /// font_size). Used to grow the line height when the atom is
        /// taller than normal text — fractions, integrals, matrices.
        em_height: f32,
        /// Distance from the top of the SVG to its alphabetic
        /// baseline, in em units. Used to paint the SVG with its
        /// baseline aligned to the surrounding text's baseline.
        em_baseline: f32,
        /// If `Some`, the SVG is painted with this color filter (so
        /// math inside a link picks up the link color, etc.).
        color: Option<Color>,
        /// If `Some`, the placeholder span gets a `.link(...)` so
        /// clicks on the rendered SVG route through the same
        /// link-click handler as surrounding text.
        link: Option<String>,
    },
}

/// Internal record per Replaced atom, parallel to its span index in
/// the widget's `spans` vec.
#[derive(Debug, Clone)]
struct AtomMeta {
    svg: Vec<u8>,
    copy_text: String,
    /// Intrinsic width in em units — used to compute paint width
    /// matching the SVG's aspect ratio so resvg doesn't squash or
    /// clip content.
    em_width: f32,
    /// Total height in em units — used to compute paint height.
    em_height: f32,
    /// Distance from top to baseline in em units — used to align
    /// the SVG's baseline with the text's baseline.
    em_baseline: f32,
    /// Optional color filter applied to the SVG at paint time.
    color: Option<Color>,
}

/// Placeholder character used to reserve horizontal space for an
/// inline replaced atom. We use digit zero ('0') for two reasons:
///
/// 1. **Atomic wrap.** Cosmic-text's word-finder treats whitespace
///    (including figure-space U+2007, despite UAX #14 calling it a
///    Glue / non-breaking class) as breakable separators between
///    words. A run of figure-spaces gets shattered across lines,
///    which makes our SVG paint twice (once per visual line).
///    Non-whitespace characters form one atomic "word" instead.
/// 2. **Predictable width.** OpenType defines U+2007 figure-space
///    as the same width as digits 0-9. So digit-width is the right
///    target, and `'0'` is a reliable representative.
const PLACEHOLDER_CHAR: char = '0';

/// Per-em width of `PLACEHOLDER_CHAR` ('0') in iced's default font.
/// Digit width varies by font but is ~0.55-0.6em for typical
/// proportional fonts (Fira Sans, Liberation Sans, etc.). Slightly
/// over-estimating means we use *fewer* placeholder chars, leaving
/// less horizontal slack around the centered SVG.
const PLACEHOLDER_EM_ADVANCE: f32 = 0.6;

/// cosmic-text splits a line's vertical box into ascent (~78% of
/// `line_height`) and descent (~22%). Math fractions/integrals/
/// matrices typically need closer to 50% descent — the SVG's bottom
/// can overflow the line's reserved descent. We pad the widget's
/// reported min_bounds.height so the parent container (e.g. a table
/// cell) grows to fit the overflow, and our `widget_bounds` clip
/// covers the rendered SVG.
const COSMIC_LINE_DESCENT_FRACTION: f32 = 0.22;

/// A rich-text block that supports drag-to-select, `selected_text()`
/// copy-collection, AND inline replaced atoms (SVGs/images embedded
/// in the text flow with proper line wrapping).
pub struct SelectableRichText<'a, Message, Theme = ::iced::Theme, Renderer = ::iced::Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    /// Spans fed to iced's `with_spans` machinery. Replaced atoms
    /// appear here as transparent placeholder runs of figure-spaces.
    spans: Vec<text::Span<'a, String, Renderer::Font>>,
    /// Side-table parallel to `spans`: `atom_meta[i] = Some(_)` means
    /// span `i` is a Replaced-atom placeholder; `None` means a normal
    /// text span.
    atom_meta: Vec<Option<AtomMeta>>,
    size: Option<Pixels>,
    line_height: LineHeight,
    width: Length,
    height: Length,
    font: Option<Renderer::Font>,
    align_x: text::Alignment,
    align_y: alignment::Vertical,
    wrapping: Wrapping,
    class: Theme::Class<'a>,
    on_link_click: Option<Box<dyn Fn(String) -> Message + 'a>>,
    selection: Selection,
}

/// Free constructor matching `iced::widget::rich_text`. Pass the same
/// [`Selection`] handle to every block in a document so they share
/// cross-block drag state.
pub fn selectable_rich_text<'a, Message, Theme, Renderer>(
    spans: Vec<text::Span<'a, String, Renderer::Font>>,
    selection: Selection,
) -> SelectableRichText<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    let n = spans.len();
    SelectableRichText {
        spans,
        atom_meta: vec![None; n],
        size: None,
        line_height: LineHeight::default(),
        width: Length::Shrink,
        height: Length::Shrink,
        font: None,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        wrapping: Wrapping::default(),
        class: <Theme as Catalog>::default(),
        on_link_click: None,
        selection,
    }
}

/// Rich-text constructor that supports inline replaced atoms (math
/// SVGs, images) alongside text spans. Atoms are rendered via
/// `iced::advanced::svg::Renderer::draw_svg` at positions allocated
/// by transparent placeholder glyphs — they participate in line
/// wrapping like browsers' inline-block elements.
pub fn selectable_rich_content<'a, Message, Theme, Renderer>(
    atoms: Vec<InlineAtom<'a, Renderer::Font>>,
    selection: Selection,
) -> SelectableRichText<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    let mut spans = Vec::with_capacity(atoms.len());
    let mut atom_meta = Vec::with_capacity(atoms.len());
    for atom in atoms {
        match atom {
            InlineAtom::Span(s) => {
                spans.push(s);
                atom_meta.push(None);
            }
            InlineAtom::Replaced {
                svg,
                copy_text,
                em_width,
                em_height,
                em_baseline,
                color,
                link,
            } => {
                // Build a non-breaking placeholder of digits wide
                // enough to occupy ≈ em_width worth of horizontal space.
                let n = ((em_width / PLACEHOLDER_EM_ADVANCE).ceil() as usize).max(1);
                let placeholder: String = std::iter::repeat(PLACEHOLDER_CHAR).take(n).collect();
                let mut placeholder_span = text::Span::new(placeholder).color(Color::TRANSPARENT);
                if let Some(href) = link.clone() {
                    placeholder_span = placeholder_span.link(href);
                }
                // Per-span line_height grows the line containing this
                // atom enough for the math; cosmic-text takes the max
                // line_height across spans on a line. Add a small
                // fixed padding (~2px above + 2px below at 16px body
                // text — 0.25em total) so the math doesn't kiss the
                // surrounding text vertically.
                const LINE_PAD_EM: f32 = 0.25;
                if em_height > 1.0 {
                    placeholder_span =
                        placeholder_span.line_height(LineHeight::Relative(em_height + LINE_PAD_EM));
                }
                spans.push(placeholder_span);
                atom_meta.push(Some(AtomMeta {
                    svg,
                    copy_text,
                    em_width,
                    em_height,
                    em_baseline,
                    color,
                }));
            }
        }
    }
    SelectableRichText {
        spans,
        atom_meta,
        size: None,
        line_height: LineHeight::default(),
        width: Length::Shrink,
        height: Length::Shrink,
        font: None,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        wrapping: Wrapping::default(),
        class: <Theme as Catalog>::default(),
        on_link_click: None,
        selection,
    }
}

impl<'a, Message, Theme, Renderer> SelectableRichText<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    pub fn on_link_click<F>(mut self, f: F) -> Self
    where
        F: Fn(String) -> Message + 'a,
    {
        self.on_link_click = Some(Box::new(f));
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

struct State<P: text::Paragraph> {
    spans: Vec<text::Span<'static, String, P::Font>>,
    /// Mirrors widget.atom_meta. Cached in state so paint and copy
    /// paths can identify atom spans without re-borrowing the widget
    /// (which can be partially-moved during update etc.).
    atom_meta: Vec<Option<AtomMeta>>,
    flat_text: String,
    span_offsets: Vec<usize>,
    /// Byte offset in `flat_text` of the start of each cosmic-text
    /// BUFFER line (text between `\n` separators in the source).
    /// `LayoutGlyph::start`/`end` and `Cursor::index` returned by
    /// cosmic-text are LINE-LOCAL byte offsets — for selection /
    /// copy in flat_text-global coords, we add `line_offsets[line_i]`
    /// to convert. Without this conversion, multi-line widgets (e.g.
    /// code blocks) get rectangular selection because line-local
    /// bytes only accidentally match the global selection range.
    line_offsets: Vec<usize>,
    paragraph: P,
    span_pressed: Option<usize>,
    hovered_link: Option<usize>,
}

impl<P: text::Paragraph> Default for State<P> {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            atom_meta: Vec::new(),
            flat_text: String::new(),
            span_offsets: Vec::new(),
            line_offsets: vec![0],
            paragraph: P::default(),
            span_pressed: None,
            hovered_link: None,
        }
    }
}

/// Non-generic state fragment exposed to operations. `Operation::custom`
/// receives `&mut dyn Any`; the concrete type must not be generic, so
/// we project into this owned snapshot before publishing.
#[derive(Debug, Clone)]
struct SelectionFragment {
    /// Top y-coordinate of the widget in window-local coords. Used to
    /// sort fragments into reading order.
    y: f32,
    text: String,
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for SelectableRichText<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer + svg::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();
        layout::sized(limits, self.width, self.height, |limits| {
            let bounds = limits.max();
            let size = self.size.unwrap_or_else(|| renderer.default_size());
            let font = self.font.unwrap_or_else(|| renderer.default_font());

            let text_with_spans = || ::iced::advanced::Text {
                content: self.spans.as_slice(),
                bounds,
                size,
                line_height: self.line_height,
                font,
                align_x: self.align_x,
                align_y: self.align_y,
                shaping: Shaping::Advanced,
                wrapping: self.wrapping,
            };

            // Reuse iced's paragraph caching strategy: hash spans by
            // equality, only re-shape when content changes.
            if state.spans.as_slice() != self.spans.as_slice() {
                state.paragraph = Renderer::Paragraph::with_spans(text_with_spans());
                state.spans = self
                    .spans
                    .iter()
                    .cloned()
                    .map(text::Span::to_static)
                    .collect();
                state.atom_meta = self.atom_meta.clone();

                // Rebuild flat_text + span_offsets for selection
                // substring extraction.
                state.flat_text.clear();
                state.span_offsets.clear();
                for span in &state.spans {
                    state.span_offsets.push(state.flat_text.len());
                    state.flat_text.push_str(&span.text);
                }
                // Rebuild line_offsets — byte offset in flat_text of
                // each cosmic-text buffer line. cosmic-text splits
                // its source on `\n` into BufferLines and indexes
                // glyph bytes / hit_test cursors WITHIN each line,
                // so we need this offset table to convert back to
                // flat_text-global bytes.
                state.line_offsets.clear();
                state.line_offsets.push(0);
                for (i, b) in state.flat_text.bytes().enumerate() {
                    if b == b'\n' {
                        state.line_offsets.push(i + 1);
                    }
                }
            } else {
                match state.paragraph.compare(::iced::advanced::Text {
                    content: (),
                    bounds,
                    size,
                    line_height: self.line_height,
                    font,
                    align_x: self.align_x,
                    align_y: self.align_y,
                    shaping: Shaping::Advanced,
                    wrapping: self.wrapping,
                }) {
                    text::Difference::None => {}
                    text::Difference::Bounds => {
                        // For atom-bearing widgets, do a full re-shape
                        // on bounds change. cosmic-text's `set_size`
                        // path re-wraps but does NOT correctly carry
                        // per-span line_height_opt onto the new visual
                        // line groupings — so atoms that wrap to a
                        // new line keep the line at the previous
                        // (stale) height. A full rebuild via
                        // `with_spans` re-derives the per-line height
                        // from scratch, fixing the stale state.
                        if state.atom_meta.iter().any(|m| m.is_some()) {
                            state.paragraph = Renderer::Paragraph::with_spans(text_with_spans());
                        } else {
                            state.paragraph.resize(bounds);
                        }
                    }
                    text::Difference::Shape => {
                        state.paragraph = Renderer::Paragraph::with_spans(text_with_spans());
                    }
                }
            }

            // Pad min_bounds.height by the worst-case descent overflow
            // across atoms — cosmic-text's line descent (~22% of
            // line_height) often isn't enough room for math fractions/
            // integrals/matrices, whose own descent ratio is closer to
            // 50%. Without this padding, tall math in tight containers
            // (table cells) gets clipped at container bottom.
            let mut min_bounds = state.paragraph.min_bounds();
            let mut max_overflow_em = 0.0_f32;
            for meta in state.atom_meta.iter().flatten() {
                let line_descent_em = COSMIC_LINE_DESCENT_FRACTION * meta.em_height;
                let math_descent_em = meta.em_height - meta.em_baseline;
                max_overflow_em = max_overflow_em.max(math_descent_em - line_descent_em);
            }
            if max_overflow_em > 0.0 {
                min_bounds.height += max_overflow_em * size.0;
            }
            min_bounds
        })
    }

    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        let style = <Theme as Catalog>::style(theme, &self.class);
        let translation = layout.position() - Point::ORIGIN;

        // 1. Paint span decorations (highlight bg, underline, strike) —
        //    same algorithm as iced::widget::Rich.
        for (index, span) in self.spans.iter().enumerate() {
            let is_hovered_link = self.on_link_click.is_some() && Some(index) == state.hovered_link;
            if !(span.highlight.is_some()
                || span.underline
                || span.strikethrough
                || is_hovered_link)
            {
                continue;
            }
            let regions = state.paragraph.span_bounds(index);
            if let Some(highlight) = span.highlight {
                for bounds in &regions {
                    let bounds = Rectangle::new(
                        bounds.position() - Vector::new(span.padding.left, span.padding.top),
                        bounds.size() + Size::new(span.padding.x(), span.padding.y()),
                    );
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: bounds + translation,
                            border: highlight.border,
                            ..Default::default()
                        },
                        highlight.background,
                    );
                }
            }
            if span.underline || span.strikethrough || is_hovered_link {
                let size = span
                    .size
                    .or(self.size)
                    .unwrap_or_else(|| renderer.default_size());
                let line_height = span
                    .line_height
                    .unwrap_or(self.line_height)
                    .to_absolute(size);
                let color = span.color.or(style.color).unwrap_or(defaults.text_color);
                let baseline =
                    translation + Vector::new(0.0, size.0 + (line_height.0 - size.0) / 2.0);
                if span.underline || is_hovered_link {
                    for bounds in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    bounds.position() + baseline - Vector::new(0.0, size.0 * 0.08),
                                    Size::new(bounds.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            color,
                        );
                    }
                }
                if span.strikethrough {
                    for bounds in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    bounds.position() + baseline - Vector::new(0.0, size.0 / 2.0),
                                    Size::new(bounds.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            color,
                        );
                    }
                }
            }
        }

        // 2. Paint selection highlight ON TOP of span backgrounds so
        //    e.g. inline-code grey backgrounds don't cover the blue.
        //    The blue uses translucent alpha so glyphs underneath
        //    remain readable. Implements UAX #9 selection painting:
        //    one rectangle per visually-contiguous run of in-selection
        //    glyphs (multiple rectangles on mixed-bidi lines).
        let bounds = layout.bounds();
        let highlight_color = Color::from_rgba(0.40, 0.60, 1.00, 0.35);
        let byte_range = match local_selection_range(&self.selection, bounds) {
            Clipped::Outside => None,
            Clipped::Whole => Some((0, state.flat_text.len())),
            Clipped::Partial(start, end) => {
                let lo = hit_test_global_byte(&state.paragraph, &state.line_offsets, start);
                let hi = hit_test_global_byte(&state.paragraph, &state.line_offsets, end);
                match (lo, hi) {
                    (Some(a), Some(b)) if a != b => {
                        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                        Some((lo, hi))
                    }
                    _ => None,
                }
            }
        };
        if let Some((lo, hi)) = byte_range {
            paint_bidi_selection(
                renderer,
                &state.paragraph,
                &state.line_offsets,
                lo,
                hi,
                translation,
                highlight_color,
            );
        }

        // 3. Paint the glyphs themselves. Atom placeholders are
        //    color = TRANSPARENT, so they paint nothing visible —
        //    we'll overlay the SVG next.
        ::iced::widget::text::draw(
            renderer,
            defaults,
            layout.bounds(),
            &state.paragraph,
            style,
            viewport,
        );

        // 4. Overlay inline replaced atoms (SVGs) onto the placeholder
        //    glyph runs. Walks cosmic-text layout runs directly,
        //    groups consecutive same-span glyphs (= one atom run),
        //    and paints the SVG at that bounding rect, baseline-
        //    aligned with the surrounding text.
        if state.atom_meta.iter().any(|a| a.is_some()) {
            paint_atoms(
                renderer,
                &state.paragraph,
                &state.atom_meta,
                translation,
                layout.bounds(),
            );
        }
    }

    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();
        let bounds = layout.bounds();

        // Update hovered link for cursor styling (only when on_link_click set).
        let was_hovered = state.hovered_link.is_some();
        if let Some(local) = cursor.position_in(bounds) {
            if self.on_link_click.is_some() {
                state.hovered_link = state.paragraph.hit_span(local).and_then(|span_idx| {
                    self.spans
                        .get(span_idx)
                        .and_then(|s| s.link.as_ref())
                        .map(|_| span_idx)
                });
            } else {
                state.hovered_link = None;
            }
        } else {
            state.hovered_link = None;
        }
        if was_hovered != state.hovered_link.is_some() {
            shell.request_redraw();
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(window_pos) = cursor.position() else {
                    return;
                };
                let inside = cursor.position_in(bounds).is_some();
                if !inside {
                    // Outside this widget — let the [`SelectionGroup`]
                    // wrapper handle press logic (start/clear). Inner
                    // widgets stay quiet so they don't fight the wrapper.
                    return;
                }
                if state.hovered_link.is_some() {
                    state.span_pressed = state.hovered_link;
                    shell.capture_event();
                    return;
                }
                self.selection.press(window_pos);
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if self.selection.is_dragging() {
                    if let Some(pos) = cursor.position() {
                        self.selection.drag_to(pos);
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                // Link click resolution.
                if let Some(handler) = &self.on_link_click {
                    if let Some(pressed) = state.span_pressed.take() {
                        if Some(pressed) == state.hovered_link {
                            if let Some(link) = self.spans.get(pressed).and_then(|s| s.link.clone())
                            {
                                shell.publish(handler(link));
                            }
                        }
                    }
                }
                self.selection.release();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        if state.hovered_link.is_some() {
            return mouse::Interaction::Pointer;
        }
        if cursor.position_in(layout.bounds()).is_some() {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }

    fn operate(
        &mut self,
        tree: &mut tree::Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation<()>,
    ) {
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        let bounds = layout.bounds();
        let text = match local_selection_range(&self.selection, bounds) {
            Clipped::Outside => return,
            Clipped::Whole if state.flat_text.is_empty() => return,
            Clipped::Whole => substitute_atoms_in_range(state, 0, state.flat_text.len()),
            Clipped::Partial(anchor, cursor) => {
                match compute_selected_text(state, anchor, cursor) {
                    Some(t) => t,
                    None => return,
                }
            }
        };
        let mut frag = SelectionFragment { y: bounds.y, text };
        operation.custom(None, bounds, &mut frag as &mut dyn Any);
    }
}

/// Read the shared selection and clip it to a widget's bounds. Returns
/// `Clipped::Outside` if the widget is fully above or below the range,
/// `Clipped::Whole` if the widget is fully enclosed (caller should copy
/// the full flat_text), or `Clipped::Partial(anchor, cursor)` for
/// widget-local coords to feed into hit_test.
///
/// We split "fully enclosed" out as its own case because hit_test at
/// the bottom-right corner of a multi-line paragraph can mis-resolve
/// (cosmic-text's hit() under iced's Hit::CharOffset doesn't reliably
/// give the trailing offset for points beyond the last glyph). For
/// fully-enclosed widgets we sidestep hit_test entirely.
enum Clipped {
    Outside,
    Whole,
    Partial(Point, Point),
}

/// Pure helper for unit-testing the per-line bidi grouping logic.
/// Takes a slice of glyphs in visual (left-to-right) order, each as
/// `(g_lo, g_hi, x_left, x_right)` where `[g_lo, g_hi)` is the glyph's
/// logical byte range and `[x_left, x_right]` is its horizontal extent.
/// Returns one `(x_start, x_end)` pair per visually-contiguous run of
/// glyphs whose byte range overlaps `[lo, hi)`.
///
/// This is the algorithmic core of `paint_bidi_selection`, extracted
/// so it can be unit-tested without an iced runtime.
fn group_in_selection_glyphs(
    glyphs: &[(usize, usize, f32, f32)],
    lo: usize,
    hi: usize,
) -> Vec<(f32, f32)> {
    /// Pixels of slack between adjacent glyph rects to still treat
    /// them as visually contiguous (subpixel layout, kerning, etc.).
    const ADJACENCY_EPSILON_PX: f32 = 0.5;

    let mut out = Vec::new();
    let mut current: Option<(f32, f32)> = None;
    for &(g_lo, g_hi, x0, x1) in glyphs {
        let in_sel = g_lo < hi && g_hi > lo;
        if in_sel {
            match current {
                Some((s, e)) if (x0 - e).abs() <= ADJACENCY_EPSILON_PX => {
                    current = Some((s, x1));
                }
                Some((s, e)) => {
                    out.push((s, e));
                    current = Some((x0, x1));
                }
                None => current = Some((x0, x1)),
            }
        } else if let Some((s, e)) = current.take() {
            out.push((s, e));
        }
    }
    if let Some((s, e)) = current {
        out.push((s, e));
    }
    out
}

fn local_selection_range(selection: &Selection, bounds: Rectangle) -> Clipped {
    let Some((start, end)) = selection.range() else {
        return Clipped::Outside;
    };
    let top = bounds.y;
    let bot = bounds.y + bounds.height;
    if end.y < top || start.y > bot {
        return Clipped::Outside;
    }
    let starts_above = start.y < top;
    let ends_below = end.y > bot;
    if starts_above && ends_below {
        return Clipped::Whole;
    }
    let local_start = if starts_above {
        Point::new(0.0, 0.0)
    } else {
        Point::new((start.x - bounds.x).max(0.0), (start.y - bounds.y).max(0.0))
    };
    let local_end = if ends_below {
        Point::new(bounds.width, bounds.height)
    } else {
        Point::new(
            (end.x - bounds.x).clamp(0.0, bounds.width),
            (end.y - bounds.y).clamp(0.0, bounds.height),
        )
    };
    Clipped::Partial(local_start, local_end)
}

fn compute_selected_text<P: text::Paragraph + 'static>(
    state: &State<P>,
    anchor: Point,
    cursor: Point,
) -> Option<String> {
    let start_idx = hit_test_global_byte(&state.paragraph, &state.line_offsets, anchor)?;
    let end_idx = hit_test_global_byte(&state.paragraph, &state.line_offsets, cursor)?;
    let (lo, hi) = if start_idx <= end_idx {
        (start_idx, end_idx)
    } else {
        (end_idx, start_idx)
    };
    if lo == hi {
        return None;
    }
    // cosmic-text's `cursor.index` (which iced surfaces as
    // `Hit::CharOffset`) is actually a byte offset within the line buffer.
    // Use it as bytes when it lands on a char boundary, otherwise fall
    // back to char-indexing.
    let lo_b = lo.min(state.flat_text.len());
    let hi_b = hi.min(state.flat_text.len());
    let raw_slice =
        if state.flat_text.is_char_boundary(lo_b) && state.flat_text.is_char_boundary(hi_b) {
            (lo_b, hi_b)
        } else {
            let mut chars = state.flat_text.char_indices();
            let lo_byte = chars
                .nth(lo)
                .map(|(b, _)| b)
                .unwrap_or(state.flat_text.len());
            let hi_byte = if hi > lo {
                chars
                    .nth(hi - lo - 1)
                    .map(|(b, _)| b)
                    .unwrap_or(state.flat_text.len())
            } else {
                lo_byte
            };
            (lo_byte, hi_byte)
        };
    Some(substitute_atoms_in_range(state, raw_slice.0, raw_slice.1))
}

/// Build the selection's clipboard string from the raw byte range
/// `[lo, hi)` in `flat_text`, substituting each Replaced-atom span's
/// placeholder bytes with its `copy_text`. This means selecting any
/// part of "Inline: $a^2+b^2=c^2$." copies the surrounding text
/// concatenated with the original LaTeX source — never the U+2007
/// placeholder runs.
fn substitute_atoms_in_range<P: text::Paragraph>(state: &State<P>, lo: usize, hi: usize) -> String {
    let mut out = String::new();
    let mut cursor = lo;
    // Build (start, end, copy_text) entries for each atom that overlaps
    // the range, sorted by start byte. Spans inherit their start from
    // span_offsets and length from the placeholder text.
    let mut atoms: Vec<(usize, usize, &str)> = state
        .atom_meta
        .iter()
        .enumerate()
        .filter_map(|(idx, meta)| {
            let meta = meta.as_ref()?;
            let start = *state.span_offsets.get(idx)?;
            let placeholder_len = state.spans.get(idx).map(|s| s.text.len())?;
            let end = start + placeholder_len;
            // Filter to atoms that overlap [lo, hi).
            if end <= lo || start >= hi {
                return None;
            }
            Some((start, end, meta.copy_text.as_str()))
        })
        .collect();
    atoms.sort_by_key(|(start, _, _)| *start);

    for (a_start, a_end, copy_text) in atoms {
        // Emit any literal flat_text bytes before this atom.
        let pre_end = a_start.max(cursor);
        if pre_end > cursor {
            out.push_str(&state.flat_text[cursor..pre_end]);
        }
        // Emit the atom's canonical source (e.g. LaTeX) once, even
        // if the user only selected part of the placeholder.
        out.push_str(copy_text);
        cursor = a_end;
    }
    if cursor < hi {
        out.push_str(&state.flat_text[cursor..hi]);
    }
    out
}

/// Walk `paragraph.buffer().layout_runs()` and overlay each Replaced
/// atom's SVG on the placeholder glyph run that occupies its byte
/// range. `atom_meta[span_idx]` is `Some` if span `span_idx` is an
/// atom; cosmic-text's `LayoutGlyph::metadata` carries the span
/// index (set by iced's `with_spans` bridge), so we can recognize
/// atom glyphs in the post-layout glyph stream.
///
/// Per visual line (one `LayoutRun`), we walk glyphs left-to-right,
/// accumulate consecutive same-atom glyphs into a bounding rect, and
/// emit a `draw_svg` call for each completed run. This produces ONE
/// SVG draw per atom per visual line — matches inline-block
/// rendering in browsers.
fn paint_atoms<R: text::Renderer + svg::Renderer>(
    renderer: &mut R,
    paragraph: &R::Paragraph,
    atom_meta: &[Option<AtomMeta>],
    translation: Vector,
    widget_bounds: Rectangle,
) where
    R::Paragraph: 'static,
{
    let any: &dyn Any = paragraph;
    let Some(ipara) = any.downcast_ref::<::iced_graphics::text::Paragraph>() else {
        return;
    };
    for run in ipara.buffer().layout_runs() {
        // cosmic-text's absolute baseline y for this visual line.
        // We paint atoms with their alphabetic baseline aligned here.
        let baseline_y = run.line_y;
        let mut current: Option<(usize, f32, f32, f32)> = None; // (span_idx, x_min, x_max, font_size)
        for glyph in run.glyphs {
            let span_idx = glyph.metadata;
            let is_atom = atom_meta.get(span_idx).and_then(|a| a.as_ref()).is_some();
            let g_x0 = glyph.x;
            let g_x1 = glyph.x + glyph.w;
            let g_size = glyph.font_size;
            match current {
                Some((idx, x_min, x_max, size)) if is_atom && idx == span_idx => {
                    current = Some((idx, x_min.min(g_x0), x_max.max(g_x1), size));
                }
                Some((idx, x_min, x_max, size)) => {
                    emit_atom_svg(
                        renderer,
                        atom_meta,
                        idx,
                        x_min,
                        x_max,
                        baseline_y,
                        size,
                        translation,
                        widget_bounds,
                    );
                    current = if is_atom {
                        Some((span_idx, g_x0, g_x1, g_size))
                    } else {
                        None
                    };
                }
                None if is_atom => {
                    current = Some((span_idx, g_x0, g_x1, g_size));
                }
                None => {}
            }
        }
        if let Some((idx, x_min, x_max, size)) = current {
            emit_atom_svg(
                renderer,
                atom_meta,
                idx,
                x_min,
                x_max,
                baseline_y,
                size,
                translation,
                widget_bounds,
            );
        }
    }
}

fn emit_atom_svg<R: svg::Renderer>(
    renderer: &mut R,
    atom_meta: &[Option<AtomMeta>],
    span_idx: usize,
    x_min: f32,
    x_max: f32,
    baseline_y: f32,
    placeholder_font_size: f32,
    translation: Vector,
    widget_bounds: Rectangle,
) {
    let Some(atom) = atom_meta.get(span_idx).and_then(|a| a.as_ref()) else {
        return;
    };
    // Paint at the SVG's NATURAL dimensions — not the placeholder
    // run's actual width. iced_wgpu's draw_svg rasterizes via resvg
    // PRESERVING ASPECT RATIO: it scales the SVG to fit one axis of
    // bounds and lets the other dimension extend to whatever fits
    // proportionally. If our bounds aspect doesn't match the SVG's,
    // the rendered tree can OVERFLOW THE PIXMAP we asked for and
    // get clipped. Painting at natural dimensions sidesteps that.
    //
    // Center horizontally within the placeholder run so any width
    // slack (placeholder digits being slightly wider than the SVG
    // due to PLACEHOLDER_EM_ADVANCE approximation) is balanced
    // left/right.
    let svg_width = atom.em_width * placeholder_font_size;
    let svg_height = atom.em_height * placeholder_font_size;
    let placeholder_width = x_max - x_min;
    let x_offset = ((placeholder_width - svg_width) / 2.0).max(0.0);
    // Empirical 2px upward bias: mathml-core's reported baseline
    // sits slightly below the text's optical baseline at typical
    // body sizes, so the math reads as "sinking" without the lift.
    const BASELINE_LIFT_PX: f32 = 0.75;
    let svg_top = baseline_y - atom.em_baseline * placeholder_font_size - BASELINE_LIFT_PX;
    let bounds = Rectangle {
        x: x_min + x_offset,
        y: svg_top,
        width: svg_width,
        height: svg_height,
    };
    let handle = svg::Handle::from_memory(atom.svg.clone());
    let mut svg_widget = svg::Svg::new(handle);
    if let Some(c) = atom.color {
        svg_widget = svg_widget.color(c);
    }
    // Clip to widget bounds so the SVG can spill slightly above /
    // below the line it's painted on without being chopped at line
    // boundaries.
    renderer.draw_svg(svg_widget, bounds + translation, widget_bounds);
}

/// Paint a selection highlight per UAX #9 / CSS Pseudo-Elements 4 §3.5.
///
/// Walks `paragraph.buffer().layout_runs()` directly via cosmic-text.
/// One `LayoutRun` per visual line; within each, glyphs are in
/// screen-left-to-right visual order and carry their logical byte
/// range (`start`/`end`) plus position (`x`, `w`). For each visual
/// line, glyphs whose byte range overlaps the selection are grouped
/// into visually-contiguous runs and painted as one rectangle each.
/// Mixed-bidi lines naturally produce multiple rectangles with gaps
/// — matching the rendering Range::getClientRects() returns in
/// browsers.
///
/// Falls back to a no-op if the concrete `Renderer::Paragraph` isn't
/// `iced_graphics::text::Paragraph` (the only impl in iced 0.14, but
/// the trait is open). Legitimate fallback: no highlight is better
/// than a wrong one.
/// Hit-test a point against the paragraph's cosmic-text Buffer
/// directly, returning a flat_text-global byte offset. iced's
/// `Hit::CharOffset` discards the line index and only reports
/// `cursor.index` which is LINE-LOCAL — for multi-line widgets
/// (code blocks) this gives the wrong byte. Reach into cosmic-text
/// via the `iced_graphics::text::Paragraph` downcast so we can use
/// the full Cursor (line + index) and add the line offset.
fn hit_test_global_byte<P: text::Paragraph + 'static>(
    paragraph: &P,
    line_offsets: &[usize],
    point: Point,
) -> Option<usize> {
    let any: &dyn Any = paragraph;
    if let Some(ipara) = any.downcast_ref::<::iced_graphics::text::Paragraph>() {
        let cursor = ipara.buffer().hit(point.x, point.y)?;
        let offset = line_offsets.get(cursor.line).copied().unwrap_or(0);
        return Some(offset + cursor.index);
    }
    // Fallback: trust iced's hit_test (will be wrong for multi-line).
    paragraph.hit_test(point).map(|h| h.cursor())
}

fn paint_bidi_selection<R: text::Renderer>(
    renderer: &mut R,
    paragraph: &R::Paragraph,
    line_offsets: &[usize],
    lo: usize,
    hi: usize,
    translation: Vector,
    color: Color,
) where
    R::Paragraph: 'static,
{
    let any: &dyn Any = paragraph;
    let Some(ipara) = any.downcast_ref::<::iced_graphics::text::Paragraph>() else {
        return;
    };
    for run in ipara.buffer().layout_runs() {
        let line_top = run.line_top;
        let line_height = run.line_height;
        // cosmic-text indexes glyph bytes within their BUFFER LINE;
        // shift by the line's offset in flat_text so the comparisons
        // against [lo, hi) (which are flat_text-global) work.
        let line_offset = line_offsets.get(run.line_i).copied().unwrap_or(0);
        let glyphs: Vec<(usize, usize, f32, f32)> = run
            .glyphs
            .iter()
            .map(|g| (g.start + line_offset, g.end + line_offset, g.x, g.x + g.w))
            .collect();
        for (x_start, x_end) in group_in_selection_glyphs(&glyphs, lo, hi) {
            let rect = Rectangle {
                x: x_start,
                y: line_top,
                width: x_end - x_start,
                height: line_height,
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: rect + translation,
                    ..Default::default()
                },
                color,
            );
        }
    }
}

impl<'a, Message, Theme, Renderer> From<SelectableRichText<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: Catalog + 'a,
    Renderer: text::Renderer + svg::Renderer + 'a,
{
    fn from(widget: SelectableRichText<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}

// ─── SelectionGroup wrapper ─────────────────────────────────────────
//
// Owns the whole rendered region, intercepts mouse events for the
// shared selection, and delegates everything else to its inner Element.
// Lets dead-space clicks (between blocks, around list bullets, etc.)
// initiate drags instead of falling through.

pub struct SelectionGroup<'a, Message, Theme, Renderer>
where
    Renderer: ::iced::advanced::Renderer,
{
    inner: Element<'a, Message, Theme, Renderer>,
    selection: Selection,
}

pub fn selection_group<'a, Message, Theme, Renderer>(
    inner: impl Into<Element<'a, Message, Theme, Renderer>>,
    selection: Selection,
) -> SelectionGroup<'a, Message, Theme, Renderer>
where
    Renderer: ::iced::advanced::Renderer,
{
    SelectionGroup {
        inner: inner.into(),
        selection,
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for SelectionGroup<'a, Message, Theme, Renderer>
where
    Renderer: ::iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.inner.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.inner.as_widget().state()
    }

    fn children(&self) -> Vec<tree::Tree> {
        self.inner.as_widget().children()
    }

    fn diff(&self, tree: &mut tree::Tree) {
        self.inner.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.inner.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.inner.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.inner.as_widget_mut().layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner
            .as_widget()
            .draw(tree, renderer, theme, defaults, layout, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Let the inner subtree process the event first so individual
        // SelectableRichText widgets can call `selection.start` from
        // their own bounds (which is the desired source for hit-tests
        // inside text). Then we step in to handle dead-space clicks.
        self.inner.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );

        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(window_pos) = cursor.position() else {
                    return;
                };
                let inside = cursor.position_in(bounds).is_some();
                if inside {
                    // Seed pending anchor for dead-space clicks. If an
                    // inner widget already pressed at the same point,
                    // its press call set the same pending anchor —
                    // this is idempotent.
                    if !self.selection.is_dragging() {
                        self.selection.press(window_pos);
                        shell.request_redraw();
                    }
                } else {
                    // True outside-the-content click: clear.
                    if !self.selection.is_dragging() {
                        self.selection.clear();
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if self.selection.is_dragging() {
                    if let Some(pos) = cursor.position() {
                        self.selection.drag_to(pos);
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                self.selection.release();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.inner
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut tree::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation<()>,
    ) {
        self.inner
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut tree::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<::iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        self.inner
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<SelectionGroup<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: ::iced::advanced::Renderer + 'a,
{
    fn from(widget: SelectionGroup<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}

// ─── selected_text Task ─────────────────────────────────────────────

fn collect_selected() -> impl Operation<Option<String>> {
    struct CollectSelected {
        fragments: Vec<(f32, String)>,
    }

    impl Operation<Option<String>> for CollectSelected {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<String>>)) {
            // Just descend — never short-circuit, never freeze a
            // partial result. We need every leaf widget in the tree
            // to push its fragment.
            operate(self);
        }

        fn custom(
            &mut self,
            _id: Option<&::iced::advanced::widget::Id>,
            _bounds: Rectangle,
            state: &mut dyn Any,
        ) {
            if let Some(frag) = state.downcast_mut::<SelectionFragment>() {
                self.fragments
                    .push((frag.y, std::mem::take(&mut frag.text)));
            }
        }

        fn finish(&self) -> Outcome<Option<String>> {
            if self.fragments.is_empty() {
                return Outcome::Some(None);
            }
            let mut sorted = self.fragments.clone();
            sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
            let joined = sorted
                .into_iter()
                .map(|(_, s)| s)
                .collect::<Vec<_>>()
                .join("\n");
            Outcome::Some(Some(joined))
        }
    }

    CollectSelected {
        fragments: Vec::new(),
    }
}

/// Walks the widget tree and returns the concatenation of every
/// [`SelectableRichText`]'s currently-selected substring, sorted
/// top-to-bottom.
///
/// Wire to a Cmd+C subscription:
///
/// ```ignore
/// match msg {
///     Message::CopyRequest => markdawn::adapter::iced::selected_text()
///         .and_then(|s| match s {
///             Some(text) => iced::clipboard::write(text),
///             None => Task::none(),
///         }),
///     _ => Task::none(),
/// }
/// ```
pub fn selected_text() -> Task<Option<String>> {
    ::iced_runtime::task::widget(collect_selected())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: drive selection past the threshold so existing tests
    /// don't have to repeat the press + drag-past dance.
    fn drag_from_to(s: &Selection, from: Point, to: Point) {
        s.press(from);
        s.drag_to(to);
    }

    #[test]
    fn selection_default_inactive() {
        let s = Selection::new();
        assert!(!s.active());
        assert!(!s.is_dragging());
        assert!(s.range().is_none());
    }

    #[test]
    fn selection_drag_lifecycle() {
        let s = Selection::new();
        s.press(Point::new(10.0, 5.0));
        // Press alone doesn't activate.
        assert!(!s.active());
        // Cross threshold: 50 - 10 = 40px way past 4.
        s.drag_to(Point::new(50.0, 25.0));
        assert!(s.active());
        assert!(s.is_dragging());
        let (start, end) = s.range().unwrap();
        // Start was top-left, end was bottom-right.
        assert_eq!(start, Point::new(10.0, 5.0));
        assert_eq!(end, Point::new(50.0, 25.0));
        s.release();
        assert!(s.active());
        assert!(!s.is_dragging());
    }

    #[test]
    fn selection_clear() {
        let s = Selection::new();
        drag_from_to(&s, Point::new(10.0, 5.0), Point::new(50.0, 25.0));
        s.clear();
        assert!(!s.active());
    }

    #[test]
    fn selection_normalizes_reverse_drag() {
        let s = Selection::new();
        drag_from_to(&s, Point::new(50.0, 25.0), Point::new(10.0, 5.0));
        let (start, end) = s.range().unwrap();
        assert_eq!(start, Point::new(10.0, 5.0));
        assert_eq!(end, Point::new(50.0, 25.0));
    }

    #[test]
    fn local_range_clips_widget_above() {
        let s = Selection::new();
        drag_from_to(&s, Point::new(0.0, 100.0), Point::new(0.0, 200.0));
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 50.0));
        assert!(matches!(
            local_selection_range(&s, bounds),
            Clipped::Outside
        ));
    }

    #[test]
    fn local_range_clips_widget_below() {
        let s = Selection::new();
        drag_from_to(&s, Point::new(0.0, 10.0), Point::new(0.0, 20.0));
        let bounds = Rectangle::new(Point::new(0.0, 100.0), Size::new(100.0, 50.0));
        assert!(matches!(
            local_selection_range(&s, bounds),
            Clipped::Outside
        ));
    }

    #[test]
    fn local_range_full_widget_inside_selection() {
        let s = Selection::new();
        drag_from_to(&s, Point::new(0.0, 0.0), Point::new(200.0, 200.0));
        let bounds = Rectangle::new(Point::new(0.0, 50.0), Size::new(100.0, 50.0));
        assert!(matches!(local_selection_range(&s, bounds), Clipped::Whole));
    }

    // ─── Drag-distance threshold ─────────────────────────────────

    #[test]
    fn press_alone_does_not_activate() {
        let s = Selection::new();
        s.press(Point::new(10.0, 5.0));
        assert!(!s.active());
        assert!(s.range().is_none());
    }

    #[test]
    fn small_drift_does_not_activate() {
        let s = Selection::new();
        s.press(Point::new(10.0, 5.0));
        // Manhattan = |11.5-10| + |5.5-5| = 2.0 — below the 4.0 threshold.
        s.drag_to(Point::new(11.5, 5.5));
        assert!(!s.active());
        assert!(s.range().is_none());
    }

    #[test]
    fn drag_past_threshold_activates() {
        let s = Selection::new();
        s.press(Point::new(10.0, 5.0));
        s.drag_to(Point::new(20.0, 5.0));
        assert!(s.is_dragging());
        let (start, end) = s.range().unwrap();
        assert_eq!(start, Point::new(10.0, 5.0));
        assert_eq!(end, Point::new(20.0, 5.0));
    }

    #[test]
    fn release_before_threshold_clears_pending() {
        let s = Selection::new();
        s.press(Point::new(10.0, 5.0));
        s.release();
        assert!(s.range().is_none());
        assert!(!s.is_dragging());
    }

    // ─── group_in_selection_glyphs (UAX #9 painting core) ───────

    #[test]
    fn grouping_pure_ltr_contiguous() {
        // Bytes monotonic with visual idx; a single contiguous run.
        let glyphs = [
            (0, 1, 0.0, 10.0),
            (1, 2, 10.0, 20.0),
            (2, 3, 20.0, 30.0),
            (3, 4, 30.0, 40.0),
        ];
        // Selection [1, 3): glyphs 1 and 2 in selection.
        assert_eq!(group_in_selection_glyphs(&glyphs, 1, 3), vec![(10.0, 30.0)],);
    }

    #[test]
    fn grouping_pure_rtl_contiguous() {
        // RTL: visual order left-to-right but bytes descending.
        let glyphs = [
            (3, 4, 0.0, 10.0), // visually leftmost = last logical
            (2, 3, 10.0, 20.0),
            (1, 2, 20.0, 30.0),
            (0, 1, 30.0, 40.0), // visually rightmost = first logical
        ];
        // Selection [1, 3): glyphs at visual idx 1 and 2 (bytes 1,2,3 overlap).
        assert_eq!(group_in_selection_glyphs(&glyphs, 1, 3), vec![(10.0, 30.0)],);
    }

    #[test]
    fn grouping_mixed_bidi_two_rects_with_gap() {
        // "Hello عربي world" simplified — visual order left to right.
        // visual idx 0..4 = LTR bytes 0..4
        // visual idx 4..8 = RTL bytes 4..8 in reverse logical order
        // visual idx 8..12 = LTR bytes 8..12
        let glyphs = [
            (0, 1, 0.0, 10.0),    // 'H' visual 0
            (1, 2, 10.0, 20.0),   // 'e' visual 1
            (2, 3, 20.0, 30.0),   // 'l' visual 2
            (3, 4, 30.0, 40.0),   // 'l' visual 3
            (7, 8, 40.0, 50.0),   // RTL last byte, visual 4
            (6, 7, 50.0, 60.0),   // visual 5
            (5, 6, 60.0, 70.0),   // visual 6
            (4, 5, 70.0, 80.0),   // RTL first byte, visual 7
            (8, 9, 80.0, 90.0),   // 'w' visual 8
            (9, 10, 90.0, 100.0), // 'o' visual 9
        ];
        // Select bytes [2, 6): "ll" in LTR + first half of RTL.
        // In selection by overlap:
        //   visual 2 ('l', byte 2) ✓
        //   visual 3 ('l', byte 3) ✓
        //   visual 4 (byte 7) ✗
        //   visual 5 (byte 6) ✗  (g_lo=6, g_hi=7; 6 < 6 false)
        //   visual 6 (byte 5) ✓
        //   visual 7 (byte 4) ✓
        //   visual 8 (byte 8) ✗
        // Two visually-contiguous groups: visual 2-3, then 6-7.
        assert_eq!(
            group_in_selection_glyphs(&glyphs, 2, 6),
            vec![(20.0, 40.0), (60.0, 80.0)],
        );
    }

    #[test]
    fn grouping_empty_selection_yields_no_rects() {
        let glyphs = [(0, 1, 0.0, 10.0), (1, 2, 10.0, 20.0)];
        // Empty range collapses to no overlap (g_hi > 0 && g_lo < 0 = false).
        assert_eq!(
            group_in_selection_glyphs(&glyphs, 0, 0),
            Vec::<(f32, f32)>::new()
        );
    }

    #[test]
    fn grouping_subpixel_gap_coalesces() {
        // Two glyphs whose x extents are 0.3 px apart should merge.
        let glyphs = [(0, 1, 0.0, 10.0), (1, 2, 10.3, 20.3)];
        assert_eq!(group_in_selection_glyphs(&glyphs, 0, 2), vec![(0.0, 20.3)],);
    }

    #[test]
    fn grouping_visual_split_in_pure_ltr_run() {
        // Pure LTR run, but selection skips a glyph in the middle —
        // could happen if the user selects two disjoint regions
        // (not currently reachable via mouse, but algorithm correctness).
        let glyphs = [
            (0, 1, 0.0, 10.0),
            (1, 2, 10.0, 20.0),
            (2, 3, 20.0, 30.0),
            (3, 4, 30.0, 40.0),
        ];
        // Hand-craft a selection that doesn't actually skip anything in
        // logical order — just verify a contiguous selection makes one rect.
        assert_eq!(group_in_selection_glyphs(&glyphs, 1, 4), vec![(10.0, 40.0)],);
    }

    #[test]
    fn drag_to_after_promotion_keeps_updating_cursor() {
        let s = Selection::new();
        s.press(Point::new(10.0, 5.0));
        s.drag_to(Point::new(20.0, 5.0)); // promote at 10px
        s.drag_to(Point::new(30.0, 7.0)); // continue updating
        let (_, end) = s.range().unwrap();
        assert_eq!(end, Point::new(30.0, 7.0));
    }
}
