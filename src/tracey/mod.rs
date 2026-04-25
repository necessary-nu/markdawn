//! Tracey-flavored markdown rendering: requirement-marker extraction,
//! frontmatter parsing, sync handler traits, and a `Document` pipeline that
//! mirrors the marq 2.x API surface used by [tracey].
//!
//! [tracey]: https://github.com/bearcove/tracey

pub mod diff;
pub mod extension;
pub mod frontmatter;
pub mod handler;
pub mod heading;
pub mod render;
pub mod req;

pub use diff::{diff_markdown, diff_markdown_inline};
pub use extension::{ReqPayload, TraceyExt};
pub use frontmatter::{
    Frontmatter, FrontmatterError, FrontmatterFormat, parse_frontmatter, strip_frontmatter,
};
pub use handler::{DefaultReqHandler, HandlerError, InlineCodeHandler, ReqHandler};
pub use heading::{Heading, slugify};
pub use render::{DocElement, Document, Paragraph, RenderError, RenderOptions, render};
pub use req::{InlineCodeSpan, ReqDefinition, ReqKind, RuleId, SourceSpan, parse_rule_id};
