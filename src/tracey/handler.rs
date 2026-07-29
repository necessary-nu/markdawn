//! Sync handler traits for tracey-style req and inline-code rendering.

use std::fmt;

use super::req::ReqDefinition;

#[derive(Debug)]
pub enum HandlerError {
    Custom(String),
}

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Custom(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for HandlerError {}

/// Render the opening and closing HTML for a requirement definition.
// [spec:mw:def:traceyext.handler.traits]
pub trait ReqHandler: Send + Sync {
    /// Opening HTML — runs before the body content. Typical output is the
    /// container `<div>` and any badges/affordances.
    fn start(&self, req: &ReqDefinition) -> Result<String, HandlerError>;

    /// Closing HTML — runs after the body content. Typical output is the
    /// matching `</div>`.
    fn end(&self, req: &ReqDefinition) -> Result<String, HandlerError>;
}

/// Customize how inline `` `code` `` spans render to HTML.
///
/// Returning `None` falls back to the default `<code>…</code>`.
// [spec:mw:def:traceyext.handler.traits]
pub trait InlineCodeHandler: Send + Sync {
    fn render(&self, code: &str) -> Option<String>;
}

/// Default `ReqHandler`: a simple anchored `<div>` wrapper, matching marq's
/// `DefaultReqHandler` shape so existing CSS selectors keep working.
pub struct DefaultReqHandler;

// [spec:mw:def:traceyext.handler.traits]
impl ReqHandler for DefaultReqHandler {
    fn start(&self, req: &ReqDefinition) -> Result<String, HandlerError> {
        Ok(format!(
            "<div class=\"req\" id=\"{anchor}\"><a class=\"req-link\" href=\"#{anchor}\" title=\"{id}\"><span>{id}</span></a>",
            anchor = req.anchor_id,
            id = req.id,
        ))
    }

    fn end(&self, _req: &ReqDefinition) -> Result<String, HandlerError> {
        Ok("</div>".to_string())
    }
}
