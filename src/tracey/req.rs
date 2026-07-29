//! Requirement-definition types for tracey-style spec markdown.
//!
//! A "req" is a blockquote whose first line, after the `> ` indent, begins with
//! a tracey marker of the form `:[<sigil?><prefix>:<id>]`. Two shapes are
//! recognised:
//!
//! - `:[r:channel.id.allocation]` — ordinary requirement (`Definition`)
//! - `:[!r:determinism]` — constraint (`Constraint`)
//!
//! The leading `:` *outside* the brackets is mandatory: it discriminates a
//! tracey marker from an unrelated `[label:value]` reference link in prose
//! and from GFM admonition keywords like `[!NOTE]`. Inside the brackets the
//! optional `!` is the constraint sigil; everything else is the
//! prefix-colon-id payload, where `<prefix>` is `[a-z0-9]+` (commonly `r`)
//! and `<id>` is the rule id (validated by [`parse_rule_id`]). The trailing
//! `+N` suffix on the id encodes the version.

use std::fmt;

/// Byte offset and length in source content.
///
/// Distinct from [`crate::SourceSpan`] (which uses `start..end`); this matches
/// the offset/length form expected by tracey for rewriting markers in place.
// [spec:mw:req:traceyext.span.absolute-offsets]
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
// [spec:mw:def:traceyext.rule-id.model]
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
//
// [>r:ref.syntax.req-id+5]
// [>r:ref.syntax.version+2]
// [>r:ref.syntax.version.implicit+2]
// [>r:markdown.constraint.id-grammar+2]
//
// [spec:mw:req:traceyext.rule-id.version-suffix]
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

/// Kind of requirement marker — distinguishes the two spec-side shapes:
///
/// - `> :[r:id]` → `Definition` (ordinary requirement)
/// - `> :[!r:id]` → `Constraint` (cross-cutting; applies to heading subtree)
// [spec:mw:def:traceyext.marker.kind]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReqKind {
    /// Ordinary requirement definition: `> :[r:ID]`. Tracked for impl/verify
    /// coverage in the usual way.
    #[default]
    Definition,
    /// Constraint: `> :[!r:ID]`. Cross-cutting property that applies to its
    /// heading subtree. Verified, not implemented.
    Constraint,
}

/// A requirement definition extracted from markdown.
// [spec:mw:def:traceyext.render.document]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReqDefinition {
    /// The requirement identifier (e.g. `channel.id.allocation`).
    pub id: RuleId,
    /// Marker kind (ordinary requirement vs constraint).
    pub kind: ReqKind,
    /// Anchor id for HTML linking. Format: `r--{id}`.
    pub anchor_id: String,
    /// Source span of just the marker (e.g. `r[channel.id.allocation]` or
    /// `r![determinism]`).
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

/// Parse a tracey blockquote marker at the start of a blockquote line.
///
/// The marker shape is `:[<sigil?><prefix>:<id>]` where the leading `:` is
/// mandatory (it's the discriminator that keeps tracey from claiming stray
/// `[label:value]` brackets in prose or unknown GFM admonition keywords like
/// `[!NOTE]`); `<sigil?>` is an optional `!` denoting a constraint;
/// `<prefix>` is `[a-z0-9]+`; and `<id>` is the rule id (validated by
/// [`parse_rule_id`]).
///
/// Returns `(prefix, kind, marker_content, marker_byte_end)` where
/// `marker_content` is the rule-id text and `marker_byte_end` is the byte
/// index of the closing `]` in `text`. The full marker spans
/// `text[0..=marker_byte_end]` and *includes* the leading `:`.
///
/// `text` is expected to be already stripped of the leading `> ` blockquote
/// indent. The legacy paragraph-form (`r[id]`) and bracket-only form
/// (`[r:id]`, no leading `:`) are **not** markers.
//
// [>r:markdown.syntax.marker+3]
// [>r:markdown.syntax.paragraph-form-rejected]
// [>r:markdown.constraint.marker+2]
//
// [spec:mw:syn:traceyext.marker.grammar]
// [spec:mw:req:traceyext.marker.non-markers]
pub(crate) fn parse_req_leading_marker(text: &str) -> Option<(&str, ReqKind, &str, usize)> {
    let bytes = text.as_bytes();
    // Leading `:` outside the brackets is mandatory.
    if bytes.first() != Some(&b':') {
        return None;
    }
    if bytes.get(1) != Some(&b'[') {
        return None;
    }
    let (kind, prefix_start) = if bytes.get(2) == Some(&b'!') {
        (ReqKind::Constraint, 3)
    } else {
        (ReqKind::Definition, 2)
    };
    let mut prefix_end = prefix_start;
    while let Some(&b) = bytes.get(prefix_end) {
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            prefix_end += 1;
        } else {
            break;
        }
    }
    if prefix_end == prefix_start {
        return None;
    }
    if bytes.get(prefix_end) != Some(&b':') {
        return None;
    }
    let id_start = prefix_end + 1;
    let close = text[id_start..].find(']')? + id_start;
    if close <= id_start {
        return None;
    }
    let prefix = &text[prefix_start..prefix_end];
    let marker_content = &text[id_start..close];
    Some((prefix, kind, marker_content, close))
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:def:traceyext.rule-id.model/test]
    #[test]
    fn parse_unversioned_rule_id() {
        let r = parse_rule_id("my.req").unwrap();
        assert_eq!(r.base, "my.req");
        assert_eq!(r.version, 0);
    }

    // [spec:mw:req:traceyext.rule-id.version-suffix/test]
    #[test]
    fn parse_versioned_rule_id() {
        let r = parse_rule_id("my.req+2").unwrap();
        assert_eq!(r.base, "my.req");
        assert_eq!(r.version, 2);
    }

    // [spec:mw:req:traceyext.rule-id.version-suffix/test]
    #[test]
    fn reject_plus_zero() {
        assert!(parse_rule_id("my.req+0").is_none());
    }

    // [spec:mw:req:traceyext.rule-id.version-suffix/test]
    #[test]
    fn reject_empty() {
        assert!(parse_rule_id("").is_none());
        assert!(parse_rule_id("+1").is_none());
        assert!(parse_rule_id("foo+").is_none());
    }

    // [spec:mw:def:traceyext.rule-id.model/test]
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

    // [spec:mw:syn:traceyext.marker.grammar/test]
    #[test]
    fn marker_extraction_definition() {
        let (prefix, kind, content, end) = parse_req_leading_marker(":[r:my.req] body").unwrap();
        assert_eq!(prefix, "r");
        assert_eq!(kind, ReqKind::Definition);
        assert_eq!(content, "my.req");
        assert_eq!(end, 10); // index of ']'
    }

    // [spec:mw:def:traceyext.marker.kind/test]
    #[test]
    fn marker_extraction_constraint() {
        let (prefix, kind, content, end) =
            parse_req_leading_marker(":[!r:determinism] body").unwrap();
        assert_eq!(prefix, "r");
        assert_eq!(kind, ReqKind::Constraint);
        assert_eq!(content, "determinism");
        assert_eq!(end, 16); // index of ']'
    }

    // [spec:mw:syn:traceyext.marker.grammar/test]
    #[test]
    fn marker_extraction_multi_char_prefix() {
        let (prefix, kind, content, _) =
            parse_req_leading_marker(":[h2:rfc.7540.streams] body").unwrap();
        assert_eq!(prefix, "h2");
        assert_eq!(kind, ReqKind::Definition);
        assert_eq!(content, "rfc.7540.streams");
    }

    // [spec:mw:req:traceyext.marker.non-markers/test]
    #[test]
    fn marker_extraction_rejects_paragraph_form() {
        // Paragraph-form `r[id]` is no longer recognised.
        assert!(parse_req_leading_marker("r[my.req] body").is_none());
        assert!(parse_req_leading_marker("r![determinism] body").is_none());
    }

    // [spec:mw:req:traceyext.marker.non-markers/test]
    #[test]
    fn marker_extraction_rejects_missing_leading_colon() {
        // The leading `:` outside the brackets is mandatory.
        assert!(parse_req_leading_marker("[r:my.req] body").is_none());
        assert!(parse_req_leading_marker("[!r:foo] body").is_none());
    }

    // [spec:mw:req:traceyext.marker.non-markers/test]
    #[test]
    fn marker_extraction_rejects_gfm_alerts() {
        // GFM alert-block tokens shouldn't trigger tracey extraction.
        assert!(parse_req_leading_marker("[!NOTE]").is_none());
        assert!(parse_req_leading_marker("[!WARNING] something").is_none());
    }

    // [spec:mw:req:traceyext.marker.non-markers/test]
    #[test]
    fn marker_extraction_rejects_unknown_sigil() {
        // Only `!` is a valid in-bracket sigil; `>`/`?` are source-only.
        assert!(parse_req_leading_marker(":[>r:foo]").is_none());
        assert!(parse_req_leading_marker(":[?r:foo]").is_none());
        assert!(parse_req_leading_marker(":[*r:foo]").is_none());
    }
}
