//! Hit-testing and copy-offset contracts, exercised against real
//! cosmic-text glyph layout.
//!
//! `iced_graphics::text::Paragraph` shapes text through a process-global
//! `FontSystem`, so a genuinely laid-out paragraph is available without a
//! window, an event loop or a GPU surface.

use super::*;

use crate::iced_shim::Font;
use crate::iced_shim::advanced::text::{Difference, Ellipsis, Hit, Text};

type RealParagraph = ::iced_graphics::text::Paragraph;

const SIZE: f32 = 16.0;

fn text_of<C>(content: C) -> Text<C, Font> {
    Text {
        content,
        bounds: Size::new(1_000.0, 1_000.0),
        size: Pixels(SIZE),
        line_height: LineHeight::default(),
        font: Font::default(),
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::default(),
        ellipsis: Ellipsis::default(),
        hint_factor: None,
    }
}

/// Byte offset in `flat_text` of the start of each cosmic-text buffer line,
/// built the way the widget's `layout` builds it.
fn line_offsets(flat_text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, b) in flat_text.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

fn laid_out(content: &str) -> (RealParagraph, Vec<usize>) {
    let paragraph = RealParagraph::with_text(text_of(content));
    (paragraph, line_offsets(content))
}

/// Visual glyph extents of one buffer line as
/// `(byte_start, byte_end, x_left, x_right)`.
fn glyphs_of(paragraph: &RealParagraph, line: usize) -> Vec<(usize, usize, f32, f32)> {
    paragraph
        .buffer()
        .layout_runs()
        .filter(|run| run.line_i == line)
        .flat_map(|run| {
            run.glyphs
                .iter()
                .map(|g| (g.start, g.end, g.x, g.x + g.w))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Mid-height y coordinate of a buffer line.
fn y_of(paragraph: &RealParagraph, line: usize) -> f32 {
    let run = paragraph
        .buffer()
        .layout_runs()
        .find(|run| run.line_i == line)
        .expect("line exists");
    run.line_top + run.line_height / 2.0
}

// ----- global byte offsets ----------------------------------------------

// [spec:mw:req:iced.hit-test.global-byte-offset/test]
#[test]
fn hit_testing_resolves_to_a_flat_text_global_byte_offset() {
    let content = "abc\ndef\nghi";
    let (paragraph, offsets) = laid_out(content);
    assert_eq!(offsets, vec![0, 4, 8]);

    // First glyph of each line, addressed in flat-text-global bytes.
    for (line, expected) in [(0usize, 0usize), (1, 4), (2, 8)] {
        let point = Point::new(0.5, y_of(&paragraph, line));
        assert_eq!(
            hit_test_global_byte(&paragraph, &offsets, point),
            Some(expected),
            "line {line} start"
        );
    }

    // Last glyph boundary of the second line: line-local 3 + offset 4.
    let last = glyphs_of(&paragraph, 1).last().copied().expect("glyphs");
    let point = Point::new(last.3 - 0.5, y_of(&paragraph, 1));
    assert_eq!(hit_test_global_byte(&paragraph, &offsets, point), Some(7));
}

// [spec:mw:req:iced.hit-test.global-byte-offset/test]
#[test]
fn iceds_own_hit_test_discards_the_line_index_and_would_select_a_rectangle() {
    // The failure mode the rule exists to prevent: a code block's second
    // line resolving to the same offsets as its first.
    let content = "abc\ndef\nghi";
    let (paragraph, offsets) = laid_out(content);

    let line_1 = Point::new(0.5, y_of(&paragraph, 1));
    let line_2 = Point::new(0.5, y_of(&paragraph, 2));

    // iced's `Hit::CharOffset` is line-local: both lines report 0.
    assert_eq!(paragraph.hit_test(line_1).map(|h| h.cursor()), Some(0));
    assert_eq!(paragraph.hit_test(line_2).map(|h| h.cursor()), Some(0));

    // Adding `line_offsets[line]` is what separates them.
    assert_eq!(hit_test_global_byte(&paragraph, &offsets, line_1), Some(4));
    assert_eq!(hit_test_global_byte(&paragraph, &offsets, line_2), Some(8));
}

// [spec:mw:sem:iced.hit-test.half-advance/test]
#[test]
fn a_point_resolves_by_which_half_of_the_glyph_it_falls_in() {
    let (paragraph, offsets) = laid_out("abc");
    let y = y_of(&paragraph, 0);
    let glyphs = glyphs_of(&paragraph, 0);
    assert_eq!(glyphs.len(), 3);

    for &(start, end, x_left, x_right) in &glyphs {
        let width = x_right - x_left;
        let leading = Point::new(x_left + width * 0.25, y);
        let trailing = Point::new(x_left + width * 0.75, y);

        assert_eq!(
            hit_test_global_byte(&paragraph, &offsets, leading),
            Some(start),
            "leading half of glyph {start}..{end} should resolve before it"
        );
        assert_eq!(
            hit_test_global_byte(&paragraph, &offsets, trailing),
            Some(end),
            "trailing half of glyph {start}..{end} should resolve after it"
        );
    }
}

// [spec:mw:sem:iced.hit-test.outside-glyphs+1/test]
#[test]
fn points_above_below_and_beside_the_glyphs_still_resolve() {
    let content = "abc\ndefgh";
    let (paragraph, offsets) = laid_out(content);

    // Above the first visual line -> offset 0 of that line.
    assert_eq!(
        hit_test_global_byte(&paragraph, &offsets, Point::new(4.0, -500.0)),
        Some(0)
    );
    // Left of the first glyph of an LTR line -> start of that line.
    assert_eq!(
        hit_test_global_byte(
            &paragraph,
            &offsets,
            Point::new(-500.0, y_of(&paragraph, 1))
        ),
        Some(4)
    );
    // Right of every glyph on a line -> greatest glyph end on that line.
    let line_0_end = glyphs_of(&paragraph, 0).last().copied().unwrap().1;
    assert_eq!(
        hit_test_global_byte(
            &paragraph,
            &offsets,
            Point::new(5_000.0, y_of(&paragraph, 0))
        ),
        Some(line_0_end)
    );
    // Below the last visual line -> end of the last line.
    assert_eq!(
        hit_test_global_byte(&paragraph, &offsets, Point::new(4.0, 5_000.0)),
        Some(content.len())
    );
}

// [spec:mw:sem:iced.hit-test.outside-glyphs+1/test]
#[test]
fn which_side_is_the_line_start_follows_the_lines_base_direction() {
    // Hebrew alef-bet-gimel: 3 code points, 2 bytes each.
    const RTL: &str = "\u{5d0}\u{5d1}\u{5d2}";

    // `far` is (offset far left, offset far right) for a one-line paragraph.
    let far = |content: &str| -> (Option<usize>, Option<usize>) {
        let (paragraph, offsets) = laid_out(content);
        let y = y_of(&paragraph, 0);
        (
            hit_test_global_byte(&paragraph, &offsets, Point::new(-5_000.0, y)),
            hit_test_global_byte(&paragraph, &offsets, Point::new(5_000.0, y)),
        )
    };
    let base_rtl = |content: &str| -> bool {
        laid_out(content)
            .0
            .buffer()
            .layout_runs()
            .next()
            .expect("a line")
            .rtl
    };

    // Base-LTR: left is the logical start, right the logical end. True for
    // pure LTR and for a mixed line whose base direction is still LTR.
    assert!(!base_rtl("abc"));
    assert_eq!(far("abc"), (Some(0), Some(3)));
    let mixed_ltr = format!("abc {RTL}");
    assert!(!base_rtl(&mixed_ltr));
    assert_eq!(far(&mixed_ltr), (Some(0), Some(mixed_ltr.len())));

    // Base-RTL: mirrored. A point far right is offset 0, NOT the end — the
    // failure this rule exists to warn about.
    assert!(base_rtl(RTL));
    assert_eq!(far(RTL), (Some(RTL.len()), Some(0)));
    let mixed_rtl = format!("{RTL} abc");
    assert!(base_rtl(&mixed_rtl));
    assert_eq!(far(&mixed_rtl), (Some(mixed_rtl.len()), Some(0)));
}

// ----- glyph order ------------------------------------------------------

// [spec:mw:req:iced.paint.bidi-runs+1/test]
#[test]
fn glyph_order_follows_the_lines_base_direction() {
    const RTL: &str = "\u{5d0}\u{5d1}\u{5d2}";

    /// `(base_rtl, glyphs)` for the single visual line of `content`.
    fn line_of(content: &str) -> (bool, Vec<(usize, usize, f32, f32)>) {
        let (paragraph, _) = laid_out(content);
        let rtl = paragraph.buffer().layout_runs().next().expect("a line").rtl;
        (rtl, glyphs_of(&paragraph, 0))
    }
    let ascending = |g: &[(usize, usize, f32, f32)]| g.windows(2).all(|w| w[0].2 < w[1].2);
    let descending = |g: &[(usize, usize, f32, f32)]| g.windows(2).all(|w| w[0].2 > w[1].2);
    let bytes_ascending = |g: &[(usize, usize, f32, f32)]| g.windows(2).all(|w| w[0].0 < w[1].0);

    // Base-LTR: boxes march rightwards. Pure LTR is the degenerate case
    // where logical and visual order coincide, which is what made
    // "screen-left-to-right visual order" look like a general rule.
    let (rtl, g) = line_of("abc");
    assert!(!rtl);
    assert!(ascending(&g) && bytes_ascending(&g));

    // Still base-LTR with an RTL run embedded: boxes keep ascending, but
    // the embedded run's byte ranges are permuted.
    let mixed_ltr = format!("abc {RTL}");
    let (rtl, g) = line_of(&mixed_ltr);
    assert!(!rtl);
    assert!(ascending(&g), "base-LTR boxes ascend in x");
    assert!(!bytes_ascending(&g), "the embedded RTL run reverses bytes");

    // Base-RTL: boxes march LEFTWARDS. The first glyph carries the first
    // bytes and sits at the greatest x; the last ends the line at x == 0.
    let (rtl, g) = line_of(RTL);
    assert!(rtl);
    assert!(descending(&g), "base-RTL boxes descend in x: {g:?}");
    assert_eq!(g.first().copied().map(|x| (x.0, x.1)), Some((0, 2)));
    assert!(g.first().unwrap().2 > g.last().unwrap().2);
    assert_eq!(g.last().unwrap().2, 0.0);

    // Base-RTL with an LTR run embedded: boxes still descend throughout,
    // and the embedded run permutes bytes.
    let mixed_rtl = format!("{RTL} abc");
    let (rtl, g) = line_of(&mixed_rtl);
    assert!(rtl);
    assert!(descending(&g), "base-RTL boxes descend in x: {g:?}");
    assert!(!bytes_ascending(&g), "the embedded LTR run reverses bytes");

    // The consequence for painting: the ascending-x precondition of
    // `group_in_selection_glyphs` does not hold for a base-RTL line, so
    // a logically contiguous selection of one is not coalesced. The
    // rectangles still tile the selected glyphs exactly, so the painted
    // region is right even though the decomposition is per-glyph.
    let (_, g) = line_of(RTL);
    let rects = group_in_selection_glyphs(&g, 0, RTL.len());
    assert_eq!(rects.len(), g.len(), "one rectangle per glyph, not one run");
    let leftmost = g.iter().map(|x| x.2).fold(f32::MAX, f32::min);
    let rightmost = g.iter().map(|x| x.3).fold(f32::MIN, f32::max);
    assert_eq!(
        rects.iter().map(|r| r.1 - r.0).sum::<f32>(),
        rightmost - leftmost,
        "the rectangles tile the glyph extent without gap or overlap"
    );
}

// ----- selected-text extraction -----------------------------------------

fn state_for(content: &str) -> State<RealParagraph> {
    let spans = vec![text::Span::new(content.to_owned())];
    State::<RealParagraph> {
        paragraph: RealParagraph::with_spans(text_of(spans.as_slice())),
        flat_text: content.to_owned(),
        span_offsets: vec![0],
        line_offsets: line_offsets(content),
        atom_meta: vec![None],
        spans,
        ..Default::default()
    }
}

// [spec:mw:req:iced.hit-test.degenerate-range/test]
#[test]
fn a_zero_width_selection_contributes_no_fragment() {
    let state = state_for("abcdef");
    let y = y_of(&state.paragraph, 0);
    let (start, _, x_left, x_right) = glyphs_of(&state.paragraph, 0)[0];
    let leading = Point::new(x_left + (x_right - x_left) * 0.25, y);

    // Both hit-tests land on the same byte offset.
    assert_eq!(
        hit_test_global_byte(&state.paragraph, &state.line_offsets, leading),
        Some(start)
    );
    assert_eq!(compute_selected_text(&state, leading, leading), None);

    // Two different points inside the same glyph half also collapse.
    let nearly = Point::new(x_left + (x_right - x_left) * 0.3, y);
    assert_eq!(compute_selected_text(&state, leading, nearly), None);

    // A real range does yield text, so the `None` above is the degenerate
    // case rather than an unconditional failure.
    let far = Point::new(5_000.0, y);
    assert_eq!(
        compute_selected_text(&state, leading, far).as_deref(),
        Some("abcdef")
    );
}

// [spec:mw:req:iced.copy.offset-interpretation/test]
#[test]
fn drag_direction_never_affects_the_extracted_text() {
    let state = state_for("abcdef");
    let y = y_of(&state.paragraph, 0);
    let glyphs = glyphs_of(&state.paragraph, 0);
    let left = Point::new(glyphs[1].2 + 0.5, y);
    let right = Point::new(glyphs[4].2 + 0.5, y);

    let forward = compute_selected_text(&state, left, right);
    let backward = compute_selected_text(&state, right, left);
    assert_eq!(forward, backward);
    assert_eq!(forward.as_deref(), Some("bcd"));
}

// [spec:mw:req:iced.copy.offset-interpretation/test]
#[test]
fn offsets_beyond_the_text_are_clamped_to_its_length() {
    let state = state_for("héllo wörld");
    let y = y_of(&state.paragraph, 0);

    // Both endpoints far outside the glyphs: clamped, not panicking.
    let all = compute_selected_text(&state, Point::new(-5_000.0, y), Point::new(5_000.0, y));
    assert_eq!(all.as_deref(), Some("héllo wörld"));

    // Drag direction is irrelevant even at the extremes.
    let reversed = compute_selected_text(&state, Point::new(5_000.0, y), Point::new(-5_000.0, y));
    assert_eq!(reversed, all);
}

// [spec:mw:req:iced.copy.offset-interpretation/test]
#[test]
fn a_mid_codepoint_offset_is_re_resolved_as_a_char_index_instead_of_panicking() {
    // A paragraph whose hit-test reports an offset that is *not* a valid
    // byte boundary in `flat_text` — the mismatch that arises because
    // cosmic-text's index is a byte offset while iced surfaces it as a
    // char offset. Here `x` chooses the reported offset.
    // 'h' 0 | 'é' 1..3 | 'l' 3 | 'l' 4 | 'o' 5   (len 6)
    let flat_text = "héllo".to_owned();
    let state = State::<ForeignParagraph> {
        paragraph: ForeignParagraph { hit_from_x: true },
        spans: vec![text::Span::new(flat_text.clone())],
        flat_text,
        span_offsets: vec![0],
        atom_meta: vec![None],
        line_offsets: vec![0],
        ..Default::default()
    };
    let at = |offset: usize| Point::new(offset as f32, 0.0);

    // 2 lands inside 'é', so both offsets are re-resolved as char indices:
    // char 1 starts at byte 1 and char 2 at byte 3, giving "é".
    assert!(!state.flat_text.is_char_boundary(2));
    assert_eq!(
        compute_selected_text(&state, at(1), at(2)).as_deref(),
        Some("é")
    );

    // When both offsets do land on boundaries they are used as bytes.
    assert_eq!(
        compute_selected_text(&state, at(3), at(5)).as_deref(),
        Some("ll")
    );

    // Reversed pairs are ordered before slicing, so direction cannot
    // change the result on either path.
    assert_eq!(
        compute_selected_text(&state, at(2), at(1)),
        compute_selected_text(&state, at(1), at(2))
    );
    assert_eq!(
        compute_selected_text(&state, at(5), at(3)),
        compute_selected_text(&state, at(3), at(5))
    );

    // Char-index resolution saturates at the end of the text rather than
    // running off it: char 2 starts at byte 3, and an unreachable upper
    // char index resolves to the end of the string.
    assert_eq!(
        compute_selected_text(&state, at(2), at(99)).as_deref(),
        Some("llo")
    );
    // Equal offsets are still the degenerate case.
    assert_eq!(compute_selected_text(&state, at(2), at(2)), None);
}

// ----- downcast fallback ------------------------------------------------

/// A `Paragraph` that is not `iced_graphics`' concrete type, so the
/// downcast in `hit_test_global_byte` fails and the fallback path runs.
#[derive(Debug, Clone, Default)]
struct ForeignParagraph {
    /// When set, `hit_test` reports the point's `x` as the resolved
    /// offset, so a test can choose any offset — including one that is
    /// not a valid byte boundary in the widget's `flat_text`.
    hit_from_x: bool,
}

impl text::Paragraph for ForeignParagraph {
    type Font = Font;

    fn with_text(_text: Text<&str, Self::Font>) -> Self {
        Self::default()
    }

    fn with_spans<Link>(_text: Text<&[text::Span<'_, Link, Self::Font>], Self::Font>) -> Self {
        Self::default()
    }

    fn resize(&mut self, _new_bounds: Size) {}

    fn compare(&self, _text: Text<(), Self::Font>) -> Difference {
        Difference::None
    }

    fn size(&self) -> Pixels {
        Pixels(SIZE)
    }

    fn hint_factor(&self) -> Option<f32> {
        None
    }

    fn font(&self) -> Self::Font {
        Font::default()
    }

    fn line_height(&self) -> LineHeight {
        LineHeight::default()
    }

    fn align_x(&self) -> text::Alignment {
        text::Alignment::Default
    }

    fn align_y(&self) -> alignment::Vertical {
        alignment::Vertical::Top
    }

    fn wrapping(&self) -> Wrapping {
        Wrapping::default()
    }

    fn ellipsis(&self) -> Ellipsis {
        Ellipsis::default()
    }

    fn shaping(&self) -> Shaping {
        Shaping::Advanced
    }

    fn bounds(&self) -> Size {
        Size::new(1_000.0, 1_000.0)
    }

    fn min_bounds(&self) -> Size {
        Size::new(10.0, 10.0)
    }

    fn hit_test(&self, point: Point) -> Option<Hit> {
        self.hit_from_x
            .then(|| Hit::CharOffset(point.x.max(0.0) as usize))
    }

    fn hit_span(&self, _point: Point) -> Option<usize> {
        None
    }

    fn span_bounds(&self, _index: usize) -> Vec<Rectangle> {
        Vec::new()
    }

    fn grapheme_position(&self, _line: usize, _index: usize) -> Option<Point> {
        None
    }
}

// [spec:mw:req:iced.hit-test.paragraph-downcast-fallback/test]
#[test]
fn a_failed_paragraph_downcast_degrades_to_iceds_own_hit_test() {
    let offsets = vec![0, 4, 8];

    // The fallback returns iced's line-local cursor unchanged — no
    // `line_offsets` are added, which is why the rule calls it wrong for
    // multi-line content, but it degrades rather than panicking.
    let foreign = ForeignParagraph { hit_from_x: true };
    assert_eq!(
        hit_test_global_byte(&foreign, &offsets, Point::new(2.0, 900.0)),
        Some(2)
    );

    // A paragraph that cannot resolve the point at all yields `None`, and
    // the callers above it then contribute no highlight and no fragment.
    let blind = ForeignParagraph { hit_from_x: false };
    assert_eq!(
        hit_test_global_byte(&blind, &offsets, Point::new(1.0, 900.0)),
        None
    );

    let state = State::<ForeignParagraph> {
        paragraph: blind,
        flat_text: "abc\ndef".to_owned(),
        line_offsets: offsets,
        ..Default::default()
    };
    assert_eq!(
        compute_selected_text(&state, Point::new(0.0, 0.0), Point::new(50.0, 50.0)),
        None,
        "no selection is a better failure than a wrong one"
    );

    // Contrast: the real paragraph type does apply the line offset.
    let (real, real_offsets) = laid_out("abc\ndef");
    assert_eq!(
        hit_test_global_byte(&real, &real_offsets, Point::new(0.5, y_of(&real, 1))),
        Some(4)
    );
}
