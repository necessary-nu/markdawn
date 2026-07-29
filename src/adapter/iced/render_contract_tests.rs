//! Configuration-surface contracts of the iced renderer: options, code
//! style, theme independence and the `Url` event shape.

use super::*;

use core::cell::Cell;

fn parse_blocks(source: &str, options: IcedOptions) -> Vec<Node<()>> {
    let mut builder = crate::parser::Parser::<()>::builder();
    if options.gfm {
        builder = builder.with_gfm();
    }
    if options.alerts {
        builder = builder.with_alerts();
    }
    let mut parser = builder.build();
    parser
        .parse_full_at(source, 0)
        .expect("parse")
        .into_iter()
        .map(|block| block.node)
        .collect()
}

// ----- options ----------------------------------------------------------

// [spec:mw:def:iced.renderer.options/test]
#[test]
fn default_options_enable_nothing_and_all_enables_everything_compiled_in() {
    let default = IcedOptions::default();
    assert!(!default.gfm);
    assert!(!default.math);
    assert!(!default.alerts);
    assert!(!default.highlights);
    assert!(!default.definition_lists);
    #[cfg(feature = "pikchr")]
    assert!(!default.pikchr);
    #[cfg(feature = "graphviz")]
    assert!(!default.graphviz);
    #[cfg(feature = "typst")]
    assert!(!default.typst);

    let all = IcedOptions::all();
    assert!(all.gfm);
    assert!(all.math);
    assert!(all.alerts);
    assert!(all.highlights);
    assert!(all.definition_lists);
    #[cfg(feature = "pikchr")]
    assert!(all.pikchr);
    #[cfg(feature = "graphviz")]
    assert!(all.graphviz);
    #[cfg(feature = "typst")]
    assert!(all.typst);

    assert_ne!(default, all);
}

// [spec:mw:def:iced.renderer.options/test]
#[test]
fn options_are_a_plain_copyable_value() {
    let options = IcedOptions::all();
    let copied = options;
    assert_eq!(copied, options);
    assert_eq!(IcedOptions::default(), IcedOptions::default());

    // Each flag is independently settable, mirroring the parser's own
    // feature flags one-for-one.
    let only_gfm = IcedOptions {
        gfm: true,
        ..IcedOptions::default()
    };
    assert!(only_gfm.gfm);
    assert!(!only_gfm.math);
    assert_ne!(only_gfm, IcedOptions::default());
    assert_ne!(only_gfm, IcedOptions::all());
}

// ----- code style -------------------------------------------------------

// [spec:mw:def:iced.renderer.code-style/test]
#[test]
fn the_code_style_carries_five_colours_and_defaults_to_a_light_palette() {
    let style = IcedCodeStyle::default();
    let luminance = |c: Color| (c.r + c.g + c.b) / 3.0;

    // Backgrounds are near-white and foregrounds near-black: a light
    // palette, which is why a dark-themed host has to override it.
    assert!(luminance(style.block_background) > 0.8, "{style:?}");
    assert!(luminance(style.inline_background) > 0.8, "{style:?}");
    assert!(luminance(style.block_foreground) < 0.2, "{style:?}");
    assert!(luminance(style.inline_foreground) < 0.2, "{style:?}");
    // The border sits between the two.
    assert!(luminance(style.block_border) > 0.5, "{style:?}");
    assert!(
        luminance(style.block_border) < luminance(style.block_background),
        "{style:?}"
    );

    // Copyable value semantics, so it can be moved into style closures.
    let copied = style;
    assert_eq!(copied, style);
    let dark = IcedCodeStyle {
        block_background: Color::BLACK,
        ..style
    };
    assert_ne!(dark, style);
}

// ----- theme independence -----------------------------------------------

// [spec:mw:req:iced.renderer.theme-independence/test]
#[test]
fn code_container_colours_come_from_the_code_style_and_not_from_a_theme() {
    // `code_block_style` takes no `Theme` at all, so it structurally
    // cannot consult one; every colour it emits is copied from the
    // caller-supplied `IcedCodeStyle`.
    let custom = IcedCodeStyle {
        block_background: Color::from_rgb(0.1, 0.2, 0.3),
        block_foreground: Color::from_rgb(0.9, 0.9, 0.9),
        block_border: Color::from_rgb(0.4, 0.5, 0.6),
        inline_background: Color::BLACK,
        inline_foreground: Color::WHITE,
    };
    let style = code_block_style(custom);
    assert_eq!(
        style.background,
        Some(Background::Color(custom.block_background))
    );
    assert_eq!(style.text_color, Some(custom.block_foreground));
    assert_eq!(style.border.color, custom.block_border);

    // Pure function: same input, same output; different input, different
    // output. Nothing ambient can move it.
    assert_eq!(
        code_block_style(custom).background,
        code_block_style(custom).background
    );
    assert_ne!(
        code_block_style(custom).background,
        code_block_style(IcedCodeStyle::default()).background
    );
}

// [spec:mw:req:iced.renderer.theme-independence/test]
#[test]
fn link_highlight_and_quote_colours_are_compile_time_constants() {
    // `const` values cannot be derived from a runtime theme, so a
    // regression that started consulting one would not compile here.
    const LINK: Color = LINK_COLOR;
    const HIGHLIGHT: Color = HIGHLIGHT_BG;
    const QUOTE: Color = QUOTE_BAR;
    assert_eq!(LINK, LINK_COLOR);
    assert_eq!(HIGHLIGHT, HIGHLIGHT_BG);
    assert_eq!(QUOTE, QUOTE_BAR);

    // They are also distinct from the code-style colours, so nothing is
    // silently sharing a theme-derived slot.
    assert_ne!(LINK_COLOR, IcedCodeStyle::default().block_foreground);
    assert_ne!(HIGHLIGHT_BG, IcedCodeStyle::default().block_background);

    // Supplying colours explicitly is the documented escape hatch.
    let dark = IcedCodeStyle {
        block_background: Color::from_rgb(0.05, 0.05, 0.06),
        block_foreground: Color::from_rgb(0.92, 0.92, 0.94),
        block_border: Color::from_rgb(0.2, 0.2, 0.22),
        inline_background: Color::from_rgb(0.1, 0.1, 0.12),
        inline_foreground: Color::from_rgb(0.92, 0.92, 0.94),
    };
    let renderer: Renderer<()> = Renderer::default().with_code_style(dark);
    let _ = renderer.render("```\nx\n```\n");
    assert_ne!(
        code_block_style(dark).background,
        code_block_style(IcedCodeStyle::default()).background
    );
}

// ----- url events -------------------------------------------------------

// [spec:mw:def:iced.renderer.url-events/test]
#[test]
fn url_carries_a_link_destination_and_a_task_toggle_request() {
    let link = Url::Link("https://example.com".to_owned());
    match &link {
        Url::Link(destination) => assert_eq!(destination, "https://example.com"),
        other => panic!("expected a link, got {other:?}"),
    }

    let toggle = Url::TaskToggle {
        id: 3,
        checked: true,
    };
    match &toggle {
        Url::TaskToggle { id, checked } => {
            assert_eq!(*id, 3u64);
            assert!(*checked);
        }
        other => panic!("expected a toggle, got {other:?}"),
    }

    // Clone + Debug, so a host can lift them into its own message enum.
    assert!(format!("{:?}", toggle.clone()).contains("TaskToggle"));
}

// [spec:mw:def:iced.renderer.url-events/test]
#[test]
fn task_toggle_ids_are_one_based_and_assigned_in_document_order() {
    let source = "- [ ] first\n- [x] second\n\n\
                  paragraph\n\n\
                  - [ ] third\n  - [x] nested fourth\n";
    let options = IcedOptions {
        gfm: true,
        ..IcedOptions::default()
    };
    let blocks = parse_blocks(source, options);

    // The counter is threaded through the whole render, so ids run
    // 1..=n across every block rather than restarting per list.
    let task_id = Cell::new(0_u64);
    let renderer: Renderer<()> = Renderer::default().with_options(options);
    for block in &blocks {
        let _ = renderer.render_node(block, &task_id);
    }
    assert_eq!(
        task_id.get(),
        4,
        "expected one id per task item across the whole document"
    );

    // A fresh render starts from 1 again, and the counter is 1-based:
    // the first task item consumes id 1, never 0.
    let single = parse_blocks("- [ ] only\n", options);
    let task_id = Cell::new(0_u64);
    let _ = renderer.render_node(&single[0], &task_id);
    assert_eq!(task_id.get(), 1);

    // Documents with no task items never advance it.
    let none = parse_blocks("just prose\n", options);
    let task_id = Cell::new(0_u64);
    let _ = renderer.render_node(&none[0], &task_id);
    assert_eq!(task_id.get(), 0);
}
