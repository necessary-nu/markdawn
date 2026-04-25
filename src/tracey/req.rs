//! Requirement-definition types for tracey-style spec markdown.
//!
//! A "req" is a paragraph (or blockquote) whose first text begins with a
//! lowercase-or-digit prefix followed by `[<id>]`, e.g. `r[channel.id.allocation]`
//! or `r[channel.id.allocation+2]`. The trailing `+N` is an optional version
//! suffix.

use std::fmt;

/// Byte offset and length in source content.
///
/// Distinct from [`crate::SourceSpan`] (which uses `start..end`); this matches
/// the offset/length form expected by tracey for rewriting markers in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct SourceSpan {
    /// Byte offset from start of content.
    pub offset: usize,
    /// Length in bytes.
    pub length: usize,
}

impl SourceSpan {
    pub const fn new(offset: usize, length: usize) -> Self {
        Self { offset, length }
    }

    pub const fn end(&self) -> usize {
        self.offset + self.length
    }
}

/// Structured rule identifier with an optional `+N` version suffix.
///
/// An unversioned id like `r[my.req]` parses to `RuleId { base, version: 0 }`.
/// `r[my.req+1]` is version 1, `r[my.req+2]` is version 2, etc. `+0` is rejected.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleId {
    /// Base identifier without version suffix.
    pub base: String,
    /// Version number. `0` means "unversioned" (the marker had no `+N`).
    pub version: u32,
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.version == 0 {
            f.write_str(&self.base)
        } else {
            write!(f, "{}+{}", self.base, self.version)
        }
    }
}

impl PartialEq<&str> for RuleId {
    fn eq(&self, other: &&str) -> bool {
        parse_rule_id(other).is_some_and(|parsed| parsed == *self)
    }
}

impl PartialEq<RuleId> for &str {
    fn eq(&self, other: &RuleId) -> bool {
        parse_rule_id(self).is_some_and(|parsed| parsed == *other)
    }
}

/// Parse a rule id with an optional `+N` version suffix.
///
/// `version: 0` means the marker had no `+N`. Explicit `+0` is rejected.
pub fn parse_rule_id(id: &str) -> Option<RuleId> {
    if id.is_empty() {
        return None;
    }

    if let Some((base, version_str)) = id.rsplit_once('+') {
        if base.is_empty() || base.contains('+') || version_str.is_empty() {
            return None;
        }
        let version = version_str.parse::<u32>().ok()?;
        if version == 0 {
            return None;
        }
        Some(RuleId {
            base: base.to_string(),
            version,
        })
    } else if id.contains('+') {
        None
    } else {
        Some(RuleId {
            base: id.to_string(),
            version: 0,
        })
    }
}

/// A requirement definition extracted from markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReqDefinition {
    /// The requirement identifier (e.g. `channel.id.allocation`).
    pub id: RuleId,
    /// Anchor id for HTML linking. Format: `r--{id}`.
    pub anchor_id: String,
    /// Source span of just the marker (e.g. `r[channel.id.allocation]`).
    pub marker_span: SourceSpan,
    /// Source span covering the marker plus its body.
    pub span: SourceSpan,
    /// 1-indexed line number where the requirement starts.
    pub line: usize,
    /// Raw markdown source of the body (without the marker). For blockquote
    /// reqs, includes the `> ` prefixes.
    pub raw: String,
    /// Rendered HTML of the body (without any wrapper from a `ReqHandler`).
    pub html: String,
}

/// An inline code span (backtick-delimited) recorded with its byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineCodeSpan {
    /// Text content inside the backticks.
    pub content: String,
    /// Source span covering the entire inline code, including the backticks.
    pub span: SourceSpan,
}

/// Strip a leading req marker from a line of text.
///
/// A marker is `[a-z0-9]+\[…\]` at the very start. Returns
/// `(prefix, marker_content, marker_byte_end)` where `marker_byte_end` is
/// the index of the closing `]` in `text`.
pub(crate) fn parse_req_leading_marker(text: &str) -> Option<(&str, &str, usize)> {
    let mut prefix_len = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            prefix_len += ch.len_utf8();
        } else {
            break;
        }
    }
    if prefix_len == 0 || text.as_bytes().get(prefix_len) != Some(&b'[') {
        return None;
    }
    let marker_end = text[prefix_len..].find(']')? + prefix_len;
    if marker_end <= prefix_len + 1 {
        return None;
    }
    let prefix = &text[..prefix_len];
    let marker_content = &text[prefix_len + 1..marker_end];
    Some((prefix, marker_content, marker_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_unversioned_rule_id() {
        let r = parse_rule_id("my.req").unwrap();
        assert_eq!(r.base, "my.req");
        assert_eq!(r.version, 0);
    }

    #[test]
    fn parse_versioned_rule_id() {
        let r = parse_rule_id("my.req+2").unwrap();
        assert_eq!(r.base, "my.req");
        assert_eq!(r.version, 2);
    }

    #[test]
    fn reject_plus_zero() {
        assert!(parse_rule_id("my.req+0").is_none());
    }

    #[test]
    fn reject_empty() {
        assert!(parse_rule_id("").is_none());
        assert!(parse_rule_id("+1").is_none());
        assert!(parse_rule_id("foo+").is_none());
    }

    #[test]
    fn display_format() {
        assert_eq!(
            format!(
                "{}",
                RuleId {
                    base: "x.y".into(),
                    version: 0
                }
            ),
            "x.y"
        );
        assert_eq!(
            format!(
                "{}",
                RuleId {
                    base: "x.y".into(),
                    version: 3
                }
            ),
            "x.y+3"
        );
    }

    #[test]
    fn marker_extraction() {
        let (prefix, content, end) = parse_req_leading_marker("r[my.req] body").unwrap();
        assert_eq!(prefix, "r");
        assert_eq!(content, "my.req");
        assert_eq!(end, 8); // index of ']'
    }
}
