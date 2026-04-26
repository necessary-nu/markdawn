//! Iced adapter — turn parsed markdown into an `iced::Element` tree.
//!
//! The submodule layout:
//! - [`render`] — the [`Renderer`], [`IcedOptions`], [`Url`] event enum,
//!   and [`RenderIcedCustom`] extension trait.
//! - [`selectable`] — the [`SelectableRichText`] widget that backs every
//!   rich-text block in the rendered output, plus the [`selected_text`]
//!   `Task` helper that the host wires up to Cmd+C.

pub mod render;
pub mod selectable;

pub use render::{IcedOptions, RenderIcedCustom, Renderer, Url};
pub use selectable::selected_text;
// Internal: SelectableRichText / selectable_rich_text are not re-exported
// because they're used only by the renderer and aren't part of the
// public API surface. Hosts compose markdown via Renderer::render.
