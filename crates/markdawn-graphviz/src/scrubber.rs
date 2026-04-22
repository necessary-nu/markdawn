//! Pre-render DOT scrubber: single-pass byte scanner that
//! (a) enforces semantic caps on the graph (node / edge / attribute /
//! subgraph count, record-label and HTML-label depth), and
//! (b) neutralises attributes that would cause libgraphviz to open
//! external files or switch layout engines on us.
//!
//! Heuristic, not a DOT parser. Errs on the side of over-counting.

use crate::{GraphvizError, MAX_BRACE_DEPTH};

// adv[impl input.node-cap]
pub const MAX_NODES: usize = 1_000;
// adv[impl input.edge-cap]
pub const MAX_EDGES: usize = 2_000;
// adv[impl input.attribute-cap]
pub const MAX_ATTRIBUTES: usize = 2_000;
// adv[impl input.subgraph-cap]
pub const MAX_SUBGRAPHS: usize = 100;
// adv[impl input.record-depth-cap]
pub const MAX_RECORD_DEPTH: usize = 16;
// adv[impl input.html-depth-cap]
pub const MAX_HTML_DEPTH: usize = 16;
// adv[impl neutralise.minlen-clamp]
pub const MAX_MINLEN: u32 = 20;

#[derive(Debug, Default)]
pub struct ScanReport {
    pub identifiers: usize,
    pub edges: usize,
    pub attributes: usize,
    pub subgraphs: usize,
    pub max_outer_depth: usize,
    pub max_record_depth: usize,
    pub max_html_depth: usize,
}

pub fn scan_and_scrub(source: &str) -> Result<(String, ScanReport), GraphvizError> {
    Scanner::new(source.as_bytes()).run()
}

struct Scanner<'a> {
    bytes: &'a [u8],
    i: usize,
    out: String,
    report: ScanReport,

    // Depth tracking
    outer_depth: usize,
    record_depth: usize,
    html_depth: usize,

    // Edge counting
    set_stack: Vec<SetFrame>,
    last_closed_set_size: usize,
    pending_edge_left: Option<usize>,
    // Was the most recent token `->` or `--`? If so, the next `{` is an
    // edge-RHS set whose close fires an edge.
    last_was_arrow: bool,
}

impl<'a> Scanner<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            i: 0,
            out: String::with_capacity(bytes.len()),
            report: ScanReport::default(),
            outer_depth: 0,
            record_depth: 0,
            html_depth: 0,
            set_stack: Vec::new(),
            last_closed_set_size: 1,
            pending_edge_left: None,
            last_was_arrow: false,
        }
    }

    fn run(mut self) -> Result<(String, ScanReport), GraphvizError> {
        while self.i < self.bytes.len() {
            let b = self.bytes[self.i];

            // Comments
            if b == b'/' && self.peek(1) == Some(b'/') {
                self.consume_line_comment();
                continue;
            }
            if b == b'/' && self.peek(1) == Some(b'*') {
                self.consume_block_comment();
                continue;
            }

            // Quoted string (potential record label inside)
            if b == b'"' {
                self.consume_string()?;
                continue;
            }

            // HTML label: `<` preceded (after whitespace) by `=` in out
            if b == b'<' && last_nonspace(&self.out) == Some(b'=') {
                self.consume_html_label()?;
                continue;
            }

            // Brace
            if b == b'{' {
                self.outer_depth += 1;
                if self.outer_depth > self.report.max_outer_depth {
                    self.report.max_outer_depth = self.outer_depth;
                }
                if self.outer_depth > MAX_BRACE_DEPTH {
                    return Err(GraphvizError::InputTooDeep(self.outer_depth));
                }
                let is_rhs = self.last_was_arrow;
                let edge_left = self.pending_edge_left.take().unwrap_or(1);
                self.set_stack.push(SetFrame {
                    size: 0,
                    is_edge_rhs: is_rhs,
                    edge_left,
                });
                self.last_was_arrow = false;
                self.out.push('{');
                self.i += 1;
                continue;
            }
            if b == b'}' {
                let frame = self.set_stack.pop().unwrap_or(SetFrame {
                    size: 0,
                    is_edge_rhs: false,
                    edge_left: 1,
                });
                let size = frame.size.max(1);
                self.last_closed_set_size = size;
                if frame.is_edge_rhs {
                    self.report.edges =
                        self.report.edges.saturating_add(frame.edge_left * size);
                }
                self.last_was_arrow = false;
                self.outer_depth = self.outer_depth.saturating_sub(1);
                self.out.push('}');
                self.i += 1;
                continue;
            }

            // Attribute list
            if b == b'[' {
                self.out.push('[');
                self.i += 1;
                continue;
            }
            if b == b']' {
                self.out.push(']');
                self.i += 1;
                continue;
            }

            // `->` or `--`
            if b == b'-' && matches!(self.peek(1), Some(b'>') | Some(b'-')) {
                if let Some(left) = self.pending_edge_left.take() {
                    // Previous edge's RHS was a single ident → factor 1.
                    self.report.edges = self.report.edges.saturating_add(left);
                }
                self.pending_edge_left = Some(self.last_closed_set_size.max(1));
                self.last_closed_set_size = 1;
                self.last_was_arrow = true;
                self.out.push('-');
                self.out.push(self.bytes[self.i + 1] as char);
                self.i += 2;
                continue;
            }

            // Identifier
            if is_ident_start(b) {
                self.consume_ident_or_attr()?;
                continue;
            }

            // Default: copy
            self.out.push(b as char);
            self.i += 1;
        }

        // Flush pending edge at EOF.
        if let Some(left) = self.pending_edge_left.take() {
            self.report.edges = self.report.edges.saturating_add(left);
        }

        Ok((self.out, self.report))
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.i + offset).copied()
    }

    fn consume_line_comment(&mut self) {
        self.out.push('/');
        self.out.push('/');
        self.i += 2;
        while self.i < self.bytes.len() && self.bytes[self.i] != b'\n' {
            self.out.push(self.bytes[self.i] as char);
            self.i += 1;
        }
    }

    fn consume_block_comment(&mut self) {
        self.out.push('/');
        self.out.push('*');
        self.i += 2;
        while self.i < self.bytes.len() {
            if self.bytes[self.i] == b'*' && self.peek(1) == Some(b'/') {
                self.out.push('*');
                self.out.push('/');
                self.i += 2;
                return;
            }
            self.out.push(self.bytes[self.i] as char);
            self.i += 1;
        }
    }

    fn consume_string(&mut self) -> Result<(), GraphvizError> {
        self.out.push('"');
        self.i += 1;
        self.record_depth = 0;
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i];
            if c == b'\\' && self.i + 1 < self.bytes.len() {
                self.out.push('\\');
                self.out.push(self.bytes[self.i + 1] as char);
                self.i += 2;
                continue;
            }
            if c == b'"' {
                self.out.push('"');
                self.i += 1;
                return Ok(());
            }
            if c == b'{' {
                self.record_depth += 1;
                if self.record_depth > self.report.max_record_depth {
                    self.report.max_record_depth = self.record_depth;
                }
                if self.record_depth > MAX_RECORD_DEPTH {
                    return Err(GraphvizError::RecordLabelTooDeep(self.record_depth));
                }
            } else if c == b'}' {
                self.record_depth = self.record_depth.saturating_sub(1);
            }
            self.out.push(c as char);
            self.i += 1;
        }
        // Unterminated — let graphviz error on it downstream.
        Ok(())
    }

    fn consume_html_label(&mut self) -> Result<(), GraphvizError> {
        // Currently at `<`.
        self.html_depth = 0;
        self.out.push('<');
        self.i += 1;
        // The HTML label extends until we see a top-level `>` that closes
        // the outer `<>` wrapping; the wrapping itself doesn't count as a
        // tag. We track tag nesting inside and exit when we see a `>`
        // with html_depth == 0 and we've just handled a tag that brought
        // it there. Simpler: consume tags until we see a top-level `>`
        // NOT part of a tag.
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i];
            if c == b'<' {
                // Nested tag open.
                let is_close = self.peek(1) == Some(b'/');
                // Find the closing `>` of this tag.
                let mut j = self.i + 1;
                while j < self.bytes.len() && self.bytes[j] != b'>' {
                    j += 1;
                }
                if j >= self.bytes.len() {
                    // Unterminated; copy rest and bail.
                    self.out
                        .push_str(core::str::from_utf8(&self.bytes[self.i..]).unwrap_or(""));
                    self.i = self.bytes.len();
                    return Ok(());
                }
                let self_closing = j > self.i + 1 && self.bytes[j - 1] == b'/';

                // Extract tag name and check for dangerous ones. Graphviz's
                // HTML-label parser supports <IMG SRC="..."/> which opens
                // a file at render time. Neutralise by renaming to an
                // unknown tag name that graphviz will error on or skip.
                let name_start = if is_close { self.i + 2 } else { self.i + 1 };
                let mut name_end = name_start;
                while name_end < self.bytes.len()
                    && self.bytes[name_end].is_ascii_alphanumeric()
                {
                    name_end += 1;
                }
                let tag_name_lower = self.bytes[name_start..name_end]
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                let is_img = tag_name_lower == b"img";

                if is_close {
                    self.html_depth = self.html_depth.saturating_sub(1);
                } else if !self_closing {
                    self.html_depth += 1;
                    if self.html_depth > self.report.max_html_depth {
                        self.report.max_html_depth = self.html_depth;
                    }
                    if self.html_depth > MAX_HTML_DEPTH {
                        return Err(GraphvizError::HtmlLabelTooDeep(self.html_depth));
                    }
                }
                if is_img {
                    // adv[impl neutralise.html-img-tag]
                    // Rename <IMG…> → <_img…> so graphviz's HTML-label
                    // parser doesn't recognise it as IMG and so doesn't
                    // try to open the SRC file.
                    self.out.push('<');
                    if is_close {
                        self.out.push('/');
                    }
                    self.out.push('_');
                    self.out.push_str(
                        core::str::from_utf8(&self.bytes[name_start..=j]).unwrap_or(""),
                    );
                } else {
                    self.out
                        .push_str(core::str::from_utf8(&self.bytes[self.i..=j]).unwrap_or(""));
                }
                self.i = j + 1;
                continue;
            }
            if c == b'>' && self.html_depth == 0 {
                // End of the outer HTML label wrapper.
                self.out.push('>');
                self.i += 1;
                return Ok(());
            }
            self.out.push(c as char);
            self.i += 1;
        }
        Ok(())
    }

    fn consume_ident_or_attr(&mut self) -> Result<(), GraphvizError> {
        let start = self.i;
        while self.i < self.bytes.len() && is_ident_body(self.bytes[self.i]) {
            self.i += 1;
        }
        let name_bytes = &self.bytes[start..self.i];
        let name = core::str::from_utf8(name_bytes).unwrap_or("");

        if name == "subgraph" {
            self.report.subgraphs = self.report.subgraphs.saturating_add(1);
            self.out.push_str(name);
            return Ok(());
        }

        // Lookahead for `=` to decide if this is an attribute name.
        let mut la = self.i;
        while la < self.bytes.len() && matches!(self.bytes[la], b' ' | b'\t' | b'\n' | b'\r') {
            la += 1;
        }
        let is_attr = la < self.bytes.len() && self.bytes[la] == b'=';

        if is_attr {
            // adv[impl input.attribute-cap] — count each name=value
            // declaration, not `[…]` lists, so a single mega-list can't
            // smuggle 100k attrs past the cap.
            self.report.attributes = self.report.attributes.saturating_add(1);
            // Don't count attr names as nodes; don't fire edges.
            let lower = name.to_ascii_lowercase();
            // adv[impl neutralise.external-file-attrs]
            // adv[impl neutralise.layout-override]
            let rename = matches!(
                lower.as_str(),
                "image"
                    | "shapefile"
                    | "fontpath"
                    | "imagepath"
                    | "layout"
                    | "stylesheet"
            );
            let is_fontname = lower == "fontname";
            let is_minlen = lower == "minlen";

            // Emit attribute name (possibly prefixed).
            if rename {
                self.out.push('_');
            }
            self.out.push_str(name);
            // Emit whitespace + `=` as-is.
            self.out
                .push_str(core::str::from_utf8(&self.bytes[self.i..=la]).unwrap_or(""));
            self.i = la + 1;

            // Skip whitespace before value (but emit it).
            while self.i < self.bytes.len()
                && matches!(self.bytes[self.i], b' ' | b'\t' | b'\n' | b'\r')
            {
                self.out.push(self.bytes[self.i] as char);
                self.i += 1;
            }

            // Read value, possibly rewriting.
            let value_start = self.i;
            if self.i < self.bytes.len() && self.bytes[self.i] == b'"' {
                // Quoted — enter string state but check value afterwards.
                let before_len = self.out.len();
                self.consume_string()?;
                let has_slash = self.out[before_len..].contains('/');
                if is_fontname && has_slash {
                    // Prepend `_` to the fontname we just emitted.
                    if let Some(pos) = self.out[..before_len].rfind("fontname") {
                        let is_already_prefixed = pos > 0 && self.out.as_bytes()[pos - 1] == b'_';
                        if !is_already_prefixed {
                            self.out.insert(pos, '_');
                        }
                    }
                }
                if is_minlen {
                    let trimmed: String = self.out[before_len..]
                        .trim_matches('"')
                        .trim()
                        .to_owned();
                    if let Ok(n) = trimmed.parse::<u32>() {
                        if n > MAX_MINLEN {
                            self.out.truncate(before_len);
                            self.out.push('"');
                            self.out.push_str(&MAX_MINLEN.to_string());
                            self.out.push('"');
                        }
                    }
                }
                return Ok(());
            } else if self.i < self.bytes.len() && self.bytes[self.i] == b'<' {
                // HTML label.
                self.consume_html_label()?;
                return Ok(());
            } else {
                // Bare token up to `,` `;` `]` whitespace.
                let out_value_start = self.out.len();
                while self.i < self.bytes.len()
                    && !matches!(
                        self.bytes[self.i],
                        b',' | b';' | b']' | b' ' | b'\t' | b'\n' | b'\r'
                    )
                {
                    self.out.push(self.bytes[self.i] as char);
                    self.i += 1;
                }
                if is_minlen {
                    let val = self.out[out_value_start..].trim().to_owned();
                    if let Ok(n) = val.parse::<u32>() {
                        if n > MAX_MINLEN {
                            self.out.truncate(out_value_start);
                            self.out.push_str(&MAX_MINLEN.to_string());
                        }
                    }
                }
                let _ = value_start;
                return Ok(());
            }
        }

        // Not an attribute — regular identifier, counts as node + set member.
        self.report.identifiers = self.report.identifiers.saturating_add(1);
        if let Some(top) = self.set_stack.last_mut() {
            top.size += 1;
        } else if let Some(left) = self.pending_edge_left.take() {
            // Outside any set: this ident is the RHS of an edge.
            self.report.edges = self.report.edges.saturating_add(left);
        }
        self.last_closed_set_size = 1;
        self.last_was_arrow = false;
        self.out.push_str(name);
        Ok(())
    }
}

#[derive(Debug)]
struct SetFrame {
    size: usize,
    is_edge_rhs: bool,
    edge_left: usize,
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_body(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn last_nonspace(s: &str) -> Option<u8> {
    s.bytes().rev().find(|b| !matches!(*b, b' ' | b'\t' | b'\n' | b'\r'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benign_passes_through() {
        let (out, r) = scan_and_scrub("digraph { A -> B }").unwrap();
        assert!(out.contains("A -> B"), "got: {out}");
        assert_eq!(r.edges, 1);
    }

    #[test]
    fn fanout_counted() {
        let src = format!(
            "digraph {{ A -> {{ {} }} }}",
            (0..50).map(|i| format!("b{i}")).collect::<Vec<_>>().join(" ")
        );
        let (_, r) = scan_and_scrub(&src).unwrap();
        assert!(r.edges >= 50, "got edges={}", r.edges);
    }

    #[test]
    fn set_to_set_counted() {
        let src = "digraph { { a b c d } -> { x y z } }";
        let (_, r) = scan_and_scrub(src).unwrap();
        assert_eq!(r.edges, 12);
    }

    #[test]
    fn image_attribute_neutralised() {
        let (out, _) = scan_and_scrub(r#"digraph { A [image="/etc/passwd"] }"#).unwrap();
        assert!(!out.contains(" image="), "got: {out}");
        assert!(!out.contains("[image="), "got: {out}");
        assert!(out.contains("_image="), "got: {out}");
    }

    #[test]
    fn stylesheet_attribute_neutralised() {
        let (out, _) = scan_and_scrub(r#"digraph { graph [stylesheet="evil.css"]; A }"#).unwrap();
        assert!(!out.contains("[stylesheet="), "got: {out}");
        assert!(out.contains("_stylesheet="), "got: {out}");
    }

    #[test]
    fn layout_attribute_neutralised() {
        let (out, _) = scan_and_scrub(r#"digraph { graph [layout=neato]; A -> B }"#).unwrap();
        assert!(!out.contains("[layout=neato"), "got: {out}");
        assert!(out.contains("_layout="), "got: {out}");
    }

    #[test]
    fn fontname_absolute_path_neutralised() {
        let (out, _) = scan_and_scrub(r#"digraph { A [fontname="/abs/Bad.ttf"] }"#).unwrap();
        assert!(!out.contains("[fontname=\"/"), "got: {out}");
        assert!(out.contains("_fontname="), "got: {out}");
    }

    #[test]
    fn fontname_logical_name_preserved() {
        let (out, _) = scan_and_scrub(r#"digraph { A [fontname="Helvetica"] }"#).unwrap();
        assert!(out.contains("fontname="), "got: {out}");
        assert!(!out.contains("_fontname"), "got: {out}");
    }

    #[test]
    fn record_label_depth_tracked_inside_string() {
        let deep = "{".repeat(20) + &"}".repeat(20);
        let src = format!(r#"digraph {{ A [shape=record, label="{deep}"] }}"#);
        let err = scan_and_scrub(&src).unwrap_err();
        assert!(matches!(err, GraphvizError::RecordLabelTooDeep(_)));
    }

    #[test]
    fn html_label_depth_tracked_inside_angle_brackets() {
        let mut src = String::from("digraph { A [label=<");
        for _ in 0..20 {
            src.push_str("<B>");
        }
        src.push_str("x");
        for _ in 0..20 {
            src.push_str("</B>");
        }
        src.push_str(">] }");
        let err = scan_and_scrub(&src).unwrap_err();
        assert!(matches!(err, GraphvizError::HtmlLabelTooDeep(_)));
    }

    #[test]
    fn html_label_img_tag_renamed() {
        let (out, _) =
            scan_and_scrub(r#"digraph { A [label=<<IMG SRC="/etc/passwd"/>>] }"#).unwrap();
        assert!(!out.contains("<IMG"), "got: {out}");
        assert!(!out.contains("<img"), "got: {out}");
        assert!(out.contains("<_"), "got: {out}");
    }

    #[test]
    fn html_label_safe_tags_preserved() {
        let (out, _) = scan_and_scrub(r#"digraph { A [label=<<B>x</B>>] }"#).unwrap();
        assert!(out.contains("<B>"), "got: {out}");
        assert!(out.contains("</B>"), "got: {out}");
    }

    #[test]
    fn subgraph_keyword_counted() {
        let src = "digraph { subgraph cluster_0 { A } subgraph cluster_1 { B } }";
        let (_, r) = scan_and_scrub(src).unwrap();
        assert_eq!(r.subgraphs, 2);
    }

    #[test]
    fn per_list_attr_count_counted() {
        // Single `[…]` list with many attrs — counter should tick for
        // each name=value, not for the list itself.
        let mut src = String::from("digraph { A [");
        for i in 0..100 {
            src.push_str(&format!("a{i}={i}, "));
        }
        src.push_str("] }");
        let (_, r) = scan_and_scrub(&src).unwrap();
        assert!(r.attributes >= 100, "got attributes={}", r.attributes);
    }

    // adv[verify neutralise.minlen-clamp]
    #[test]
    fn minlen_clamped() {
        let (out, _) = scan_and_scrub("digraph { A -> B [minlen=9999] }").unwrap();
        assert!(!out.contains("9999"), "got: {out}");
        assert!(out.contains("minlen=20"), "got: {out}");
    }
}
