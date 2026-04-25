//! HTML pass-through policy applied to every `Node::Html` an adapter writes.
//!
//! Wraps the W3C-style [`sanitizer`] crate so adapters use a real HTML
//! parser instead of byte-level scanning. Two modes ship out of the box:
//!
//! - [`HtmlFilter::safe`] — sanitize via [`sanitizer::SanitizerConfig::safe_default`]
//!   (the default).
//! - [`HtmlFilter::unrestricted`] — pass HTML through verbatim. Use only when
//!   the input is fully trusted.
//!
//! For finer control, build your own [`sanitizer::SanitizerConfig`] and pass
//! it via [`HtmlFilter::custom`].

use sanitizer::SanitizerConfig;

/// HTML pass-through policy. Cheap to clone (the inner config is `Clone`).
#[derive(Debug, Clone)]
pub struct HtmlFilter(Mode);

#[derive(Debug, Clone)]
enum Mode {
    /// Run input through sanitizer with the given config.
    Sanitize(SanitizerConfig),
    /// Trust the input — emit it verbatim.
    Unrestricted,
}

impl Default for HtmlFilter {
    fn default() -> Self {
        Self::safe()
    }
}

impl HtmlFilter {
    /// Sanitize using the W3C-aligned safe defaults.
    pub fn safe() -> Self {
        Self(Mode::Sanitize(SanitizerConfig::safe_default()))
    }

    /// Pass HTML through unchanged. Use only when the source is trusted.
    pub fn unrestricted() -> Self {
        Self(Mode::Unrestricted)
    }

    /// Sanitize using a caller-supplied [`SanitizerConfig`].
    pub fn custom(config: SanitizerConfig) -> Self {
        Self(Mode::Sanitize(config))
    }

    /// True if the filter is the unrestricted pass-through.
    pub fn is_unrestricted(&self) -> bool {
        matches!(self.0, Mode::Unrestricted)
    }

    /// Apply the policy.
    ///
    /// On sanitizer failure (e.g. invalid config — shouldn't happen for the
    /// builtin profiles) the input is dropped and an empty string is
    /// returned, mirroring the safer fallback.
    pub fn filter(&self, html: &str) -> String {
        match &self.0 {
            Mode::Unrestricted => html.to_string(),
            Mode::Sanitize(config) => match config.sanitize(html) {
                Ok(out) => out.into_inner(),
                Err(_) => String::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_strips_script() {
        let out = HtmlFilter::safe().filter("<p>ok</p><script>alert(1)</script>");
        assert!(!out.contains("<script>"));
        assert!(out.contains("<p>ok</p>"));
    }

    #[test]
    fn unrestricted_passes_em_dash_unchanged() {
        // Regression: the previous byte-level filter panicked when its
        // tag-name window crossed a UTF-8 boundary like an em dash.
        let input = "<code>x</code> — long dash here";
        assert_eq!(HtmlFilter::unrestricted().filter(input), input);
    }

    #[test]
    fn safe_handles_em_dash_without_panic() {
        let input = "<code>x</code> — long dash here";
        let out = HtmlFilter::safe().filter(input);
        assert!(out.contains("—"));
    }
}
