//! Selection-handle, clipping, copy-collection and inline-atom contracts
//! of the selectable rich-text widget.
//!
//! Every helper here is a pure function or an owned value; nothing in this
//! file needs a window, an event loop or a GPU.

use super::*;

// ----- shared selection handle ------------------------------------------

// [spec:mw:def:iced.widget.selection-handle/test]
#[test]
fn cloning_the_handle_shares_one_selection_state() {
    let owner = Selection::new();
    let block_a = owner.clone();
    let block_b = owner.clone();

    // A drag driven through one clone is visible through the others, which
    // is what makes a cross-block drag one contiguous selection.
    block_a.press(Point::new(10.0, 5.0));
    block_a.drag_to(Point::new(90.0, 200.0));

    assert!(block_b.active());
    assert!(block_b.is_dragging());
    assert_eq!(owner.range(), block_b.range());
    let (start, end) = block_b.range().expect("range");
    assert_eq!((start.x, start.y), (10.0, 5.0));
    assert_eq!((end.x, end.y), (90.0, 200.0));

    // Release through a third view finalizes for everyone.
    block_b.release();
    assert!(!owner.is_dragging());
    assert!(owner.active());

    // Clearing through any view clears for all.
    block_a.clear();
    assert!(!owner.active());
    assert!(block_b.range().is_none());
}

// [spec:mw:def:iced.widget.selection-handle/test]
#[test]
fn the_handle_stores_window_coordinates_and_orders_the_two_points() {
    let selection = Selection::new();
    // Drag upwards and leftwards: the handle normalises to (start, end) in
    // document order rather than press order.
    selection.press(Point::new(200.0, 300.0));
    selection.drag_to(Point::new(20.0, 40.0));

    let (start, end) = selection.range().expect("range");
    assert!(start.y <= end.y, "{start:?} .. {end:?}");
    assert_eq!((start.x, start.y), (20.0, 40.0));
    assert_eq!((end.x, end.y), (200.0, 300.0));

    // Nothing is stored per widget: a fresh handle sees no selection.
    assert!(Selection::new().range().is_none());
}

// ----- per-widget clipping ----------------------------------------------

fn clipped(start: Point, end: Point, bounds: Rectangle) -> Clipped {
    let selection = Selection::new();
    selection.press(start);
    selection.drag_to(end);
    local_selection_range(&selection, bounds)
}

fn widget_bounds() -> Rectangle {
    Rectangle {
        x: 10.0,
        y: 100.0,
        width: 200.0,
        height: 50.0,
    }
}

// [spec:mw:req:iced.selection.clip-partial-local/test]
#[test]
fn a_partial_selection_is_converted_to_widget_local_coordinates() {
    let bounds = widget_bounds();
    // Both endpoints inside the widget: the bounds origin is subtracted
    // from each.
    let Clipped::Partial(start, end) =
        clipped(Point::new(30.0, 110.0), Point::new(60.0, 140.0), bounds)
    else {
        panic!("expected Partial");
    };
    assert_eq!((start.x, start.y), (20.0, 10.0));
    assert_eq!((end.x, end.y), (50.0, 40.0));
}

// [spec:mw:req:iced.selection.clip-partial-local/test]
#[test]
fn a_selection_starting_above_contributes_the_local_origin() {
    let bounds = widget_bounds();
    let Clipped::Partial(start, end) =
        clipped(Point::new(999.0, 10.0), Point::new(60.0, 140.0), bounds)
    else {
        panic!("expected Partial");
    };
    assert_eq!((start.x, start.y), (0.0, 0.0));
    assert_eq!((end.x, end.y), (50.0, 40.0));
}

// [spec:mw:req:iced.selection.clip-partial-local/test]
#[test]
fn a_selection_ending_below_contributes_the_bottom_right_corner() {
    let bounds = widget_bounds();
    let Clipped::Partial(start, end) =
        clipped(Point::new(30.0, 110.0), Point::new(-999.0, 900.0), bounds)
    else {
        panic!("expected Partial");
    };
    assert_eq!((start.x, start.y), (20.0, 10.0));
    assert_eq!((end.x, end.y), (bounds.width, bounds.height));
}

// [spec:mw:req:iced.selection.clip-partial-local/test]
#[test]
fn the_start_is_floored_but_not_clamped_at_the_far_edge() {
    let bounds = widget_bounds();
    // Start x far to the right of the widget: deliberately left
    // unclamped so the hit-test's past-the-last-glyph rule resolves it.
    let Clipped::Partial(start, end) =
        clipped(Point::new(5_000.0, 110.0), Point::new(60.0, 140.0), bounds)
    else {
        panic!("expected Partial");
    };
    assert_eq!(start.x, 4_990.0, "start x must not be clamped to the box");
    assert!(start.x > bounds.width);
    // Both axes of the start are floored at zero, and the end is clamped
    // into the box on both axes.
    let Clipped::Partial(start, end2) =
        clipped(Point::new(-40.0, 110.0), Point::new(9_000.0, 149.0), bounds)
    else {
        panic!("expected Partial");
    };
    assert_eq!(start.x, 0.0);
    assert_eq!(end2.x, bounds.width);
    assert!(end.y <= bounds.height);
}

// ----- copy collection --------------------------------------------------

fn drive_collect(fragments: Vec<(f32, &str)>) -> Option<String> {
    let mut operation = collect_selected();
    for (y, text) in fragments {
        let mut fragment = SelectionFragment {
            y,
            text: text.to_owned(),
        };
        operation.custom(None, Rectangle::default(), &mut fragment as &mut dyn Any);
    }
    match operation.finish() {
        Outcome::Some(value) => value,
        _ => panic!("expected an outcome"),
    }
}

// [spec:mw:def:iced.copy.fragment/test]
#[test]
fn a_fragment_carries_the_widget_top_and_its_selected_substring() {
    let mut fragment = SelectionFragment {
        y: 42.5,
        text: "selected".to_owned(),
    };
    assert_eq!(fragment.y, 42.5);
    assert_eq!(fragment.text, "selected");

    // The type is non-generic precisely so it survives the trip through
    // `Operation::custom`'s `&mut dyn Any`.
    let erased: &mut dyn Any = &mut fragment;
    let recovered = erased
        .downcast_mut::<SelectionFragment>()
        .expect("SelectionFragment must be recoverable from &mut dyn Any");
    assert_eq!(recovered.text, "selected");

    // A generic widget state would not be: the operation only ever sees a
    // fragment published by a widget, never the widget's own state.
    let mut unrelated = 7u32;
    let other: &mut dyn Any = &mut unrelated;
    assert!(other.downcast_mut::<SelectionFragment>().is_none());
}

// [spec:mw:req:iced.copy.document-order-join/test]
#[test]
fn fragments_are_sorted_by_widget_top_and_joined_with_one_newline() {
    // Published out of document order; the outcome is top-to-bottom.
    let joined = drive_collect(vec![(300.0, "third"), (100.0, "first"), (200.0, "second")]);
    assert_eq!(joined.as_deref(), Some("first\nsecond\nthird"));
    // Exactly one newline between blocks: no blank line, no prefix.
    let joined = joined.unwrap();
    assert!(!joined.contains("\n\n"), "{joined:?}");
    assert!(!joined.starts_with('\n') && !joined.ends_with('\n'));
}

// [spec:mw:req:iced.copy.document-order-join/test]
#[test]
fn widgets_sharing_a_top_edge_keep_widget_tree_order() {
    // Side-by-side table cells, or a list marker beside its item: the sort
    // is stable, so publication order survives.
    assert_eq!(
        drive_collect(vec![(50.0, "left"), (50.0, "right"), (50.0, "end")]).as_deref(),
        Some("left\nright\nend")
    );
    assert_eq!(
        drive_collect(vec![(50.0, "end"), (50.0, "right"), (50.0, "left")]).as_deref(),
        Some("end\nright\nleft")
    );
}

// [spec:mw:req:iced.copy.document-order-join/test]
#[test]
fn a_traversal_that_collected_nothing_yields_none_not_an_empty_string() {
    assert_eq!(drive_collect(Vec::new()), None);
    // One empty fragment is still a fragment, so it is not None.
    assert_eq!(drive_collect(vec![(0.0, "")]).as_deref(), Some(""));
}

// [spec:mw:req:iced.copy.document-order-join/test]
#[test]
fn the_traversal_descends_without_short_circuiting() {
    // `traverse` hands the operation itself to the callback and returns —
    // it never substitutes a finished/partial operation, so every leaf in
    // the subtree still gets to publish.
    let mut operation = collect_selected();
    let mut descents = 0usize;
    operation.traverse(&mut |inner| {
        descents += 1;
        let mut fragment = SelectionFragment {
            y: descents as f32,
            text: format!("leaf{descents}"),
        };
        inner.custom(None, Rectangle::default(), &mut fragment as &mut dyn Any);
    });
    assert_eq!(descents, 1);
    match operation.finish() {
        Outcome::Some(Some(text)) => assert_eq!(text, "leaf1"),
        other => panic!("expected a collected fragment, got {other:?}"),
    }
}

// [spec:mw:def:iced.copy.selected-text-task/test]
#[test]
fn selected_text_is_a_widget_operation_task_over_an_optional_string() {
    // The public entry point's type is the contract: a `Task` the host
    // pipes into the clipboard, resolving to `Option<String>`.
    let task: Task<Option<String>> = selected_text();
    drop(task);

    // It is a thin wrapper over the same operation exercised above, and
    // the `Selection` handle it reads is not part of the public surface —
    // the only way in is this task.
    let outcome = drive_collect(vec![(10.0, "a"), (20.0, "b")]);
    assert_eq!(outcome.as_deref(), Some("a\nb"));
}

// ----- widget shape -----------------------------------------------------

// [spec:mw:def:iced.widget.selectable-rich-text/test]
#[test]
fn a_selectable_rich_text_is_a_span_vector_plus_a_shared_selection() {
    let selection = Selection::new();
    let spans = vec![
        text::Span::new("one ".to_owned()),
        text::Span::new("two".to_owned()).link("https://e.com".to_owned()),
    ];
    let widget: TestWidget<'static> = selectable_rich_text(spans, selection.clone());

    // One rich-text block: the spans as given, in order, with no atom
    // metadata beside them.
    assert_eq!(widget.spans.len(), 2);
    assert_eq!(widget.spans[0].text, "one ");
    assert_eq!(widget.spans[1].link.as_deref(), Some("https://e.com"));
    assert_eq!(widget.atom_meta.len(), 2);
    assert!(widget.atom_meta.iter().all(Option::is_none));

    // Link-click handling is opt-in; the drag-select and fragment-publishing
    // behaviour comes from the shared handle, which is not per-widget state.
    assert!(widget.on_link_click.is_none());
    let widget = widget.on_link_click(|_href| ());
    assert!(widget.on_link_click.is_some());

    // Sizing is configurable but defaults to shrink-to-fit.
    let widget = widget.width(Length::Fill).height(Length::Fixed(40.0));
    assert_eq!(widget.width, Length::Fill);
    assert_eq!(widget.height, Length::Fixed(40.0));

    // The widget's selection is the caller's handle, not a copy: a drag
    // driven through the original is visible to the widget.
    selection.press(Point::new(0.0, 0.0));
    selection.drag_to(Point::new(100.0, 100.0));
    assert!(widget.selection.active());
}

// [spec:mw:def:iced.widget.selection-group/test]
#[test]
fn a_selection_group_delegates_its_widget_methods_to_the_inner_element() {
    let inner = || -> Element<'static, (), crate::iced_shim::Theme, crate::iced_shim::Renderer> {
        crate::iced_shim::widget::text("inner content")
            .width(Length::Fixed(120.0))
            .into()
    };

    let bare = inner();
    let wrapped = selection_group(inner(), Selection::new());

    // Transparent wrapper: the methods that need no renderer report exactly
    // what the inner element reports, so wrapping cannot change layout.
    assert_eq!(
        Widget::size(&wrapped),
        Widget::size(bare.as_widget()),
        "the wrapper must not change the inner element's sizing"
    );
    assert!(Widget::tag(&wrapped) == Widget::tag(bare.as_widget()));

    // It owns the whole region on behalf of one shared selection.
    let selection = Selection::new();
    let group = selection_group(inner(), selection.clone());
    selection.press(Point::new(5.0, 5.0));
    selection.drag_to(Point::new(80.0, 80.0));
    assert!(group.selection.active());
    assert_eq!(group.selection.range(), selection.range());
}

// ----- inline atoms -----------------------------------------------------

type TestWidget<'a> =
    SelectableRichText<'a, (), crate::iced_shim::Theme, crate::iced_shim::Renderer>;

fn replaced_atom(
    copy_text: &str,
    em_width: f32,
    em_height: f32,
) -> InlineAtom<'static, crate::iced_shim::Font> {
    InlineAtom::Replaced {
        svg: b"<svg/>".to_vec(),
        copy_text: copy_text.to_owned(),
        em_width,
        em_height,
        em_baseline: em_height * 0.5,
        color: None,
        link: None,
    }
}

fn content_widget(atoms: Vec<InlineAtom<'static, crate::iced_shim::Font>>) -> TestWidget<'static> {
    selectable_rich_content(atoms, Selection::new())
}

// [spec:mw:def:iced.widget.inline-atom/test]
#[test]
fn an_inline_atom_is_either_a_span_or_a_replaced_svg_with_metrics() {
    let widget = content_widget(vec![
        InlineAtom::Span(text::Span::new("before ".to_owned())),
        replaced_atom("x^2", 1.0, 1.0),
        InlineAtom::Span(text::Span::new(" after".to_owned())),
    ]);

    // A `Span` atom passes through as a normal text span with no metadata;
    // a `Replaced` atom becomes a placeholder span with metadata beside it.
    assert_eq!(widget.spans.len(), 3);
    assert!(widget.atom_meta[0].is_none());
    assert!(widget.atom_meta[2].is_none());
    let meta = widget.atom_meta[1].as_ref().expect("atom metadata");
    assert_eq!(meta.copy_text, "x^2");
    assert_eq!(meta.svg, b"<svg/>");
    assert_eq!(meta.em_width, 1.0);
    assert_eq!(meta.em_height, 1.0);
    assert_eq!(meta.em_baseline, 0.5);
    assert!(meta.color.is_none());

    // The atom occupies space in the inline flow, between its neighbours.
    assert_eq!(widget.spans[0].text, "before ");
    assert!(!widget.spans[1].text.is_empty());
    assert_eq!(widget.spans[2].text, " after");

    // A link on the atom rides on the placeholder span.
    let linked = content_widget(vec![InlineAtom::Replaced {
        svg: Vec::new(),
        copy_text: "c".to_owned(),
        em_width: 1.0,
        em_height: 1.0,
        em_baseline: 0.5,
        color: Some(Color::BLACK),
        link: Some("https://e.com".to_owned()),
    }]);
    assert_eq!(linked.spans[0].link.as_deref(), Some("https://e.com"));
    assert_eq!(
        linked.atom_meta[0].as_ref().unwrap().color,
        Some(Color::BLACK)
    );
}

// [spec:mw:req:iced.atom.placeholder-run/test]
#[test]
fn a_replaced_atom_reserves_space_with_a_transparent_run_of_zeroes() {
    for (em_width, expected) in [
        (0.0_f32, 1_usize),
        (0.1, 1),
        (0.6, 1),
        (0.61, 2),
        (1.0, 2),
        (1.2, 2),
        (1.21, 3),
        (6.0, 10),
    ] {
        let widget = content_widget(vec![replaced_atom("c", em_width, 1.0)]);
        let placeholder = &widget.spans[0].text;
        assert_eq!(
            placeholder.chars().count(),
            expected,
            "em_width {em_width} should reserve {expected} placeholder chars"
        );
        assert!(
            placeholder.chars().all(|c| c == PLACEHOLDER_CHAR),
            "placeholder {placeholder:?} is not a run of PLACEHOLDER_CHAR"
        );
        // The run must not be whitespace: cosmic-text's word finder would
        // break it across visual lines and paint the SVG once per line.
        assert!(!placeholder.chars().any(char::is_whitespace));
        assert_eq!(widget.spans[0].color, Some(Color::TRANSPARENT));
    }

    // The advance is over-estimated on purpose, yielding fewer characters
    // than the true digit width would.
    assert_eq!(PLACEHOLDER_CHAR, '0');
    assert_eq!(PLACEHOLDER_EM_ADVANCE, 0.6);
    let ceil = |w: f32| ((w / PLACEHOLDER_EM_ADVANCE).ceil() as usize).max(1);
    assert_eq!(ceil(3.0), 5);
    assert!(ceil(3.0) < ((3.0_f32 / 0.55).ceil() as usize));
}

// [spec:mw:req:iced.atom.line-box-growth/test]
#[test]
fn an_atom_taller_than_one_em_carries_a_grown_per_span_line_height() {
    // At or below one em the atom fits the ordinary line box, so no
    // per-span override is set.
    for em_height in [0.5_f32, 1.0] {
        let widget = content_widget(vec![replaced_atom("c", 1.0, em_height)]);
        assert!(
            widget.spans[0].line_height.is_none(),
            "unexpected line-height override at em_height {em_height}"
        );
    }

    // Above one em the span asks for `em_height + 0.25` em, so cosmic-text
    // — which takes the maximum across the spans on a line — grows the
    // line the atom lands on.
    for em_height in [1.01_f32, 2.0, 3.5] {
        let widget = content_widget(vec![replaced_atom("c", 1.0, em_height)]);
        match widget.spans[0].line_height {
            Some(LineHeight::Relative(factor)) => {
                assert!(
                    (factor - (em_height + 0.25)).abs() < f32::EPSILON,
                    "em_height {em_height} produced line height {factor}"
                );
                assert!(factor > em_height, "the atom would touch the text above");
            }
            other => panic!("expected a relative line height, got {other:?}"),
        }
    }

    // cosmic-text reserves only 22% of the line height as descent, which
    // is what the widget's own min-height padding compensates for.
    assert_eq!(COSMIC_LINE_DESCENT_FRACTION, 0.22);
    let tall = replaced_atom("c", 1.0, 3.0);
    let InlineAtom::Replaced {
        em_height,
        em_baseline,
        ..
    } = tall
    else {
        panic!("expected a replaced atom")
    };
    let reserved = COSMIC_LINE_DESCENT_FRACTION * em_height;
    let needed = em_height - em_baseline;
    assert!(
        needed > reserved,
        "a 3em atom's descent ({needed}em) should exceed the reserved {reserved}em"
    );
}

// ----- atom substitution on copy ----------------------------------------

/// Build the widget state tables the way `layout` does, so the pure copy
/// helpers can be driven without a renderer.
fn atom_state(parts: Vec<(&str, Option<&str>)>) -> State<::iced_graphics::text::Paragraph> {
    let mut state = State::<::iced_graphics::text::Paragraph>::default();
    for (rendered, copy_text) in parts {
        state.span_offsets.push(state.flat_text.len());
        state.flat_text.push_str(rendered);
        state.spans.push(text::Span::new(rendered.to_owned()));
        state.atom_meta.push(copy_text.map(|copy_text| AtomMeta {
            svg: Vec::new(),
            copy_text: copy_text.to_owned(),
            em_width: 1.0,
            em_height: 1.0,
            em_baseline: 0.5,
            color: None,
        }));
    }
    state.line_offsets = vec![0];
    for (i, b) in state.flat_text.bytes().enumerate() {
        if b == b'\n' {
            state.line_offsets.push(i + 1);
        }
    }
    state
}

// [spec:mw:req:iced.copy.atom-substitution/test]
#[test]
fn an_overlapped_atom_contributes_its_copy_text_exactly_once() {
    // "Inline: " + placeholder("0000") + "."
    let state = atom_state(vec![
        ("Inline: ", None),
        ("0000", Some("$a^2+b^2=c^2$")),
        (".", None),
    ]);
    let whole = substitute_atoms_in_range(&state, 0, state.flat_text.len());
    assert_eq!(whole, "Inline: $a^2+b^2=c^2$.");
    assert!(
        !whole.contains('0'),
        "placeholder bytes reached the clipboard"
    );

    // Selecting only part of the placeholder run still emits the atom's
    // canonical source, once.
    for (lo, hi) in [(8, 9), (9, 11), (8, 12), (7, 10)] {
        let out = substitute_atoms_in_range(&state, lo, hi);
        assert_eq!(
            out.matches("$a^2+b^2=c^2$").count(),
            1,
            "range {lo}..{hi} produced {out:?}"
        );
        assert!(!out.contains('0'), "range {lo}..{hi} produced {out:?}");
    }
}

// [spec:mw:req:iced.copy.atom-substitution/test]
#[test]
fn atoms_are_filtered_by_half_open_overlap_and_interleaved_in_order() {
    let state = atom_state(vec![
        ("a", None),
        ("00", Some("<M1>")),
        ("b", None),
        ("00", Some("<M2>")),
        ("c", None),
    ]);
    // flat_text = "a" "00" "b" "00" "c" -> offsets 0,1,3,4,6
    assert_eq!(state.flat_text, "a00b00c");
    assert_eq!(state.span_offsets, vec![0, 1, 3, 4, 6]);

    // Literal text between the atoms is interleaved in start order.
    assert_eq!(
        substitute_atoms_in_range(&state, 0, 7),
        "a<M1>b<M2>c".to_owned()
    );

    // Half-open filtering: a range ending exactly at an atom's start does
    // not include it, and one starting exactly at its end does not either.
    assert_eq!(substitute_atoms_in_range(&state, 0, 1), "a");
    assert_eq!(substitute_atoms_in_range(&state, 3, 4), "b");
    // Touching one byte of the first atom pulls it in whole.
    assert_eq!(substitute_atoms_in_range(&state, 0, 2), "a<M1>");
    // A range covering only the second atom yields only its copy text.
    assert_eq!(substitute_atoms_in_range(&state, 4, 6), "<M2>");
}

// [spec:mw:req:iced.copy.atom-substitution/test]
#[test]
fn a_selection_with_no_atoms_is_the_literal_text() {
    let state = atom_state(vec![("plain text only", None)]);
    assert_eq!(substitute_atoms_in_range(&state, 0, 5), "plain");
    assert_eq!(
        substitute_atoms_in_range(&state, 0, state.flat_text.len()),
        "plain text only"
    );
    assert_eq!(substitute_atoms_in_range(&state, 6, 6), "");
}
