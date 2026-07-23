//! Markdawn iced adapter demo:
//! 1. Drip-feeds the markdown source byte-by-byte so you can watch every
//!    node type settle into place during streaming.
//! 2. Once fully revealed, halts so you can drag-select any text and
//!    Cmd+C to copy. (Drip-feed reparses each tick — that clobbers
//!    selection mid-stream — so we pause once the source is whole.)
//! 3. Press the "replay" button to drip-feed again from byte 0.
//!
//! Run with: `cargo run --example iced_demo --features iced`

use std::time::{Duration, Instant};

use iced::advanced::widget::Id;
use iced::widget::scrollable::{RelativeOffset, Viewport};
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Event, Length, Subscription, Task, event, keyboard};

use markdawn::{IcedOptions, IcedRenderer, IcedUrl, selected_text};

const SAMPLE: &str = include_str!("sample.md");

const TICK_MS: u64 = 30;
const BYTES_PER_TICK: usize = 4;

#[derive(Debug, Clone)]
enum Message {
    Tick(Instant),
    Replay,
    CopyRequest,
    Copied(Option<String>),
    FromMarkdown(IcedUrl),
    Scrolled(Viewport),
}

struct State {
    cursor: usize,
    running: bool,
    log: String,
    renderer: IcedRenderer,
    scroll_id: Id,
    /// Snap to bottom on every drip-feed tick when true. Cleared when
    /// the user scrolls up; re-enabled when the user scrolls back to
    /// the bottom (within EDGE_THRESHOLD).
    stick_to_bottom: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            cursor: 0,
            running: true,
            log: String::from("(streaming… select + Cmd+C once fully revealed)"),
            renderer: build_renderer(),
            scroll_id: Id::unique(),
            stick_to_bottom: true,
        }
    }
}

/// Treat scroll positions within this fraction of the bottom edge as
/// "still at bottom" — re-arms stick_to_bottom when the user scrolls
/// back near the end.
const EDGE_THRESHOLD: f32 = 0.02;

/// Build the IcedRenderer with all the options enabled by current
/// compile-time features. Wires the syntax highlighter when the
/// `highlight` feature is on.
fn build_renderer() -> IcedRenderer {
    let r = IcedRenderer::default().with_options(IcedOptions {
        gfm: true,
        math: true,
        alerts: true,
        highlights: true,
        definition_lists: true,
        #[cfg(feature = "pikchr")]
        pikchr: true,
        #[cfg(feature = "graphviz")]
        graphviz: true,
        #[cfg(feature = "typst")]
        typst: true,
        ..Default::default()
    });
    #[cfg(feature = "highlight")]
    let r = r.with_highlighter(arborium::Highlighter::new());
    r
}

fn advance_to_char_boundary(s: &str, mut idx: usize) -> usize {
    let bytes = s.as_bytes();
    while idx < bytes.len() && (bytes[idx] & 0b1100_0000) == 0b1000_0000 {
        idx += 1;
    }
    idx
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Tick(_now) => {
            if !state.running {
                return Task::none();
            }
            if state.cursor >= SAMPLE.len() {
                state.running = false;
                state.log =
                    String::from("done — drag to select any text, Cmd+C to copy, or press replay");
                return Task::none();
            }
            let target = (state.cursor + BYTES_PER_TICK).min(SAMPLE.len());
            state.cursor = advance_to_char_boundary(SAMPLE, target);
            // After advancing the drip cursor, if the user is still
            // pinned to the bottom, snap to end. This catches "the
            // content just grew taller because a code block / pikchr
            // SVG / math block finally rendered" — without it, the
            // viewport stays at the previous scroll offset and the
            // newly-revealed content scrolls out of sight.
            if state.stick_to_bottom {
                return iced::widget::operation::snap_to_end(state.scroll_id.clone());
            }
        }
        Message::Replay => {
            state.cursor = 0;
            state.running = true;
            state.log = String::from("(streaming…)");
            state.stick_to_bottom = true;
            return iced::widget::operation::snap_to(
                state.scroll_id.clone(),
                RelativeOffset::START,
            );
        }
        Message::CopyRequest => {
            return selected_text().map(Message::Copied);
        }
        Message::Copied(None) => {
            state.log = String::from("Cmd+C pressed but nothing selected");
        }
        Message::Copied(Some(s)) => {
            let preview: String = s.chars().take(60).collect();
            state.log = format!("copied {} chars: {preview}…", s.len());
            return iced::clipboard::write(s).discard();
        }
        Message::FromMarkdown(IcedUrl::Link(url)) => {
            state.log = format!("link clicked: {url}");
        }
        Message::FromMarkdown(IcedUrl::TaskToggle { id, checked }) => {
            state.log = format!("task {id} → {checked}");
        }
        Message::FromMarkdown(_) => {}
        Message::Scrolled(viewport) => {
            // Re-arm stick_to_bottom when the user scrolls within
            // EDGE_THRESHOLD of the end; otherwise (they scrolled
            // up) leave them alone.
            let at_end = viewport.relative_offset().y >= 1.0 - EDGE_THRESHOLD;
            state.stick_to_bottom = at_end;
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let visible = &SAMPLE[..state.cursor];
    let body = state.renderer.render(visible).map(Message::FromMarkdown);

    let progress = format!(
        "[{}/{} bytes — {}]",
        state.cursor,
        SAMPLE.len(),
        if state.running { "streaming" } else { "done" },
    );
    let bar = row![
        text(progress).size(11),
        button(text("replay").size(11))
            .padding(4)
            .on_press(Message::Replay),
    ]
    .spacing(8);

    let log = container(text(state.log.clone()).size(12))
        .padding(8)
        .width(Length::Fill);

    container(
        scrollable(column![body, bar, log].spacing(12).padding(16))
            .id(state.scroll_id.clone())
            .on_scroll(Message::Scrolled),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn subscription(state: &State) -> Subscription<Message> {
    let copy = event::listen_with(|event, _status, _id| match event {
        Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            let is_c = matches!(key.as_ref(), keyboard::Key::Character("c"));
            if is_c && modifiers.command() {
                Some(Message::CopyRequest)
            } else {
                None
            }
        }
        _ => None,
    });
    let mut subs = vec![copy];
    if state.running {
        subs.push(iced::time::every(Duration::from_millis(TICK_MS)).map(Message::Tick));
    }
    Subscription::batch(subs)
}

fn boot() -> (State, Task<Message>) {
    (State::default(), Task::none())
}

fn title(_state: &State) -> String {
    "markdawn iced demo".to_string()
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title(title)
        .subscription(subscription)
        .run()
}
