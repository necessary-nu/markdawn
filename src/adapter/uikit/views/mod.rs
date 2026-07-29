pub(crate) mod blockquote;
pub(crate) mod code_block;
#[cfg(feature = "graphviz")]
pub(crate) mod graphviz;
#[cfg(feature = "pikchr")]
pub(crate) mod pikchr;
pub(crate) mod table;
pub(crate) mod thematic_break;
#[cfg(feature = "typst")]
pub(crate) mod typst;

#[cfg(any(feature = "graphviz", feature = "pikchr", feature = "typst"))]
mod diagram;
#[cfg(any(feature = "graphviz", feature = "pikchr", feature = "typst"))]
pub(crate) use diagram::diagram_image_view;
