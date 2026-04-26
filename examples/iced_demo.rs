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

const SAMPLE: &str = r##"# Markdawn iced demo

This window is being **drip-fed** byte-by-byte. Each tick the rendered
view re-parses the visible prefix — so you can watch every node type
settle into place as the bytes arrive. Once fully revealed, drip-feed
stops and the text becomes selectable — drag to highlight, Cmd+C to copy.

## Heading levels (1-6)

# H1
## H2
### H3
#### H4
##### H5
###### H6

## Inline marks

Plain, **bold**, *italic*, ***bold italic***, ~~strikethrough~~,
`inline code`, ==highlight==, H<sub>2</sub>O subscript, E=mc<sup>2</sup> superscript,
a [link](https://example.com), an autolink <https://iced.rs>, and an
inline ![image-alt](pic.png).

A link with complex children: [**bold** with `code` and $\hbar$ math](https://iced.rs).

Inline image (32×32 SVG data URI):
![red](data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2232%22%20height%3D%2232%22%3E%3Crect%20width%3D%2232%22%20height%3D%2232%22%20fill%3D%22%23d33%22%2F%3E%3C%2Fsvg%3E)

## Lists

Tight unordered:

- one
- two
- three

Loose ordered:

1. first

2. second

3. third

Numbered list starting from 5:

5. fifth (the parser carries `start=5` through to the renderer)
6. sixth
7. seventh

Loose with multi-paragraph items:

- First item, first paragraph.

  Second paragraph of the same item — should still be visually
  inside the list bullet's column.

- Second item with $math$ inline.

  And a `fenced` thought.

Nested:

- outer
  - inner
    - deeper
- back

Task list:

- [x] wire markdawn into iced
- [x] cover every node variant
- [ ] polish the styling
- [ ] ship it

## Code

```rust
fn greet(name: &str) -> String {
    format!("hi, {name}")
}
```

Highlight (with arborium):

```python
def greet(name: str) -> str:
    print(f"hi, {name}")
    return name.upper()
```

```typescript
interface Greeting<T> {
    name: T;
    formal: boolean;
}

const hello = <T,>(g: Greeting<T>): string => `hi, ${g.name}`;
```

## Blockquotes

> A blockquote with **bold** and a [link](https://example.com).
>
> > And a nested one.

Rich blockquote — math, code, list, and another quote inside:

> Here's a quote with multiple paragraphs and **rich** content.
>
> A formula: $E = mc^2$, an `inline_code` snippet, then a list:
>
> - first item
> - second item with `code`
> - third item
>
> ```rust
> // even a fenced block inside the quote
> let answer = 42;
> ```
>
> > And a nested quote with $\pi^2/6$ math.

## Alerts (GFM)

> [!NOTE]
> Useful information that users should know.

> [!TIP]
> Helpful advice for doing things better.

> [!IMPORTANT]
> Crucial information necessary for users to succeed.

> [!WARNING]
> Urgent info that needs immediate attention.

> [!CAUTION]
> Negative potential consequences of an action.

## Definition lists

Markdown
: a lightweight markup language with plain-text formatting

cosmic-text
: a pure-Rust, no_std-compatible text shaper used by iced

UAX #9
: the Unicode Bidirectional Algorithm — defines how runs of differing
  directionality are reordered for display

## Tables

| left  | center | right |
|:------|:------:|------:|
| a     |   b    |     c |
| one   |  two   | three |

Cells with rich content (code, links, math, formatting):

| feature | example | size |
|:--------|:--------|-----:|
| code    | `let x = 5;` | small |
| link    | [iced](https://iced.rs) | medium |
| math    | $\pi r^2$ | $A$ |
| **bold formatting** | *italic + `code`* | ~~strike~~ |

## Math

Single inline: $a^2 + b^2 = c^2$.

Display math (block, `$$...$$`):

$$
\int_{a}^{b} f(x)\,dx = F(b) - F(a)
$$

Multiple in one paragraph (wrap stress): The Pythagorean identity
$\sin^2\theta + \cos^2\theta = 1$ relates to Euler's formula
$e^{i\pi} + 1 = 0$, while the quadratic formula
$x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$ solves $ax^2 + bx + c = 0$ — and
the Gaussian integral $\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}$
shows up everywhere. Let's also throw in $\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}$
for good measure.

Tiny vs huge: $x$, $\pi$, and then a wide one
$\det\begin{pmatrix}a&b\\c&d\end{pmatrix} = ad - bc$ on the same line.

Math inside formatting: **bold with $E = mc^2$ math**, *italic with
$\nabla \cdot \mathbf{E} = \rho/\epsilon_0$ math*, and a
[link with $\hbar = h/2\pi$ math](https://example.com).

Math inside a list:

- First law: $F = ma$
- Conservation: $\Delta E = 0$
- Schrödinger: $i\hbar \frac{\partial}{\partial t}|\psi\rangle = \hat{H}|\psi\rangle$

Math inside a blockquote:

> The most beautiful equation: $e^{i\pi} + 1 = 0$. Five fundamental
> constants in one line.

Math next to RTL: Arabic with embedded math العلاقة $a^2 + b^2 = c^2$ معروفة جدًا.

A linguistic phrase structure tree for "the cat saw the dog":

```graphviz
digraph tree {
    bgcolor="transparent";
    node  [shape=plaintext, fontname="Helvetica", fontsize=14];
    edge  [arrowhead=none];
    rankdir=TB;
    nodesep=0.25;
    ranksep=0.35;

    S    [label="S"];
    NP1  [label="NP"];
    VP   [label="VP"];
    D1   [label="D"];
    N1   [label="N"];
    The1 [label="the"];
    Cat  [label="cat"];
    V    [label="V"];
    NP2  [label="NP"];
    Saw  [label="saw"];
    D2   [label="D"];
    N2   [label="N"];
    The2 [label="the"];
    Dog  [label="dog"];

    S   -> { NP1 VP };
    NP1 -> { D1 N1 };
    D1  -> The1;
    N1  -> Cat;
    VP  -> { V NP2 };
    V   -> Saw;
    NP2 -> { D2 N2 };
    D2  -> The2;
    N2  -> Dog;
}
```

The same tree via Typst + the bundled `syntree` package — the
spiritual successor to LaTeX's `forest`:

```typst
#import "@preview/syntree:0.2.1": syntree
#syntree("[S [NP [D the] [N cat]] [VP [V saw] [NP [D the] [N dog]]]]")
```

A small cetz drawing — proves the bundled `cetz` + `oxifmt` packages
resolve and that arbitrary Typst graphics work, not just trees:

```typst
#import "@preview/cetz:0.5.0"
#cetz.canvas({
  import cetz.draw: *
  circle((0, 0), radius: 0.6, fill: rgb("#d33"))
  circle((1.4, 0), radius: 0.6, fill: rgb("#3d3"))
  circle((0.7, 1.1), radius: 0.6, fill: rgb("#33d"))
  line((0, 0), (1.4, 0), (0.7, 1.1), close: true)
})
```

Math in a table:

| name | formula |
|:-----|:--------|
| area | $A = \pi r^2$ |
| volume | $V = \frac{4}{3}\pi r^3$ |
| force | $F = G\frac{m_1 m_2}{r^2}$ |

## Footnotes

A claim[^1] and another[^note] with a back-ref to[^1] again.

[^1]: First footnote body.
[^note]: Named footnote body with **bold**.

## Multilingual paragraphs

Arabic: الحمد لله رب العالمين، هذا نص عربي طويل لاختبار التفاف الأسطر
ومحاذاة النص من اليمين إلى اليسار في عارض ماركداون. علينا التأكد من أن
النقر والسحب يعمل عبر عدة أسطر بصرية.

Korean: 안녕하세요. 이것은 한국어 단락이며, 줄 바꿈과 자소 경계 처리를
시험하기 위해 일부러 길게 작성되었습니다. 마우스를 끌어 여러 시각적
줄에 걸쳐 텍스트를 선택할 수 있어야 합니다.

Japanese: こんにちは、これは日本語の段落です。行の折り返しと書記素クラ
スターの境界処理をテストするために、わざと長くしています。複数の視覚的
な行にまたがってドラッグでテキストを選択できるはずです。

Chinese: 你好，这是一段用于测试换行与字形簇边界的中文文字。我们故意把
它写得足够长，以便能够跨越多条视觉行进行拖拽选择。希望渲染结果看起
来令人满意。

Mongolian (Cyrillic): Сайн байна уу. Энэ бол Монгол хэлээр бичсэн урт
догол мөр бөгөөд олон харагдах мөрөөр чирж сонгох тестэд зориулагдсан.

Mongolian (traditional): ᠮᠣᠩᠭᠣᠯ ᠬᠡᠯᠡ ᠨᠢ ᠨᠢᠭᠡ ᠠᠶᠢᠮᠠᠭ ᠦᠨ ᠠᠯᠲᠠᠶᠢ ᠶᠢᠨ ᠬᠡᠯᠡ ᠶᠤᠮ ᠃

Elder Futhark: ᚠᚢᚦᚨᚱᚲ ᚷᚹᚺᚾᛁᛃ ᛇᛈᛉᛊ ᛏᛒᛖᛗ ᛚᛜᛞᛟ — six aetts of runes for
the determined paste-tester.

Linear A (Bronze-Age Crete, undeciphered, just for the lulz):
𐘀 𐘁 𐘂 𐘃 𐘄 𐘅 𐘆 𐘇 𐘈 𐘉 𐘊 𐘋 𐘌 𐘍 𐘎 𐘏

Mixed bidi: Hello عربي world — and the reverse direction: שלום world עולם.
Drag from "Hello" through the Arabic into "world" and watch for multiple
highlight rectangles with gaps — that's UAX #9 selection painting working.

## Raw HTML passthrough

The HTML filter strips dangerous tags but allows safe ones. The
following inline HTML should pass through (text only — markdawn's
iced renderer doesn't render arbitrary HTML, only safe semantic tags
like `<sub>`/`<sup>`):

<div>A div block — should appear as filtered text.</div>

A paragraph with <span>an inline span</span> mixed in.

## URL safety

These should NOT become clickable: [js](javascript:alert(1)),
[data](data:text/html;base64,PHM+), [vb](vbscript:msgbox).

---

That's everything. Drag to select any of this text, then press Cmd+C.
"##;

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
            return iced::clipboard::write::<Message>(s);
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
