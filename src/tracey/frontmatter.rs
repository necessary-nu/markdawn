//! Frontmatter parsing for tracey-style spec markdown.
//!
//! Supports both YAML (`---`) and TOML (`+++`) frontmatter formats.

use std::fmt;

/// Parsed frontmatter from a markdown document.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct Frontmatter {
    /// Document title.
    pub title: String,
    /// Sort weight for ordering documents.
    pub weight: i32,
    /// Document description.
    pub description: Option<String>,
    /// Template hint (consumed by some static-site tooling).
    pub template: Option<String>,
}

/// Type of frontmatter delimiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrontmatterFormat {
    /// TOML frontmatter delimited by `+++`.
    #[default]
    Toml,
    /// YAML frontmatter delimited by `---`.
    Yaml,
}

/// Result of stripping frontmatter from a document without parsing it.
#[derive(Debug, Clone)]
pub struct StrippedFrontmatter<'a> {
    /// Raw frontmatter content (without delimiters).
    pub raw: Option<&'a str>,
    /// Markdown body following the frontmatter.
    pub body: &'a str,
    /// Detected format.
    pub format: Option<FrontmatterFormat>,
    /// Byte length of the consumed frontmatter region in the source (delimiters
    /// included). `body == &source[consumed..]`.
    pub consumed: usize,
}

#[derive(Debug)]
pub enum FrontmatterError {
    Parse(String),
}

impl fmt::Display for FrontmatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "frontmatter parse error: {msg}"),
        }
    }
}

impl std::error::Error for FrontmatterError {}

/// Strip frontmatter from a markdown document without parsing it.
pub fn strip_frontmatter(markdown: &str) -> StrippedFrontmatter<'_> {
    let (delimiter, format) = if markdown.starts_with("+++\n") || markdown.starts_with("+++\r\n") {
        ("+++", FrontmatterFormat::Toml)
    } else if markdown.starts_with("---\n") || markdown.starts_with("---\r\n") {
        ("---", FrontmatterFormat::Yaml)
    } else {
        return StrippedFrontmatter {
            raw: None,
            body: markdown,
            format: None,
            consumed: 0,
        };
    };

    let start_offset = if markdown.starts_with(&format!("{delimiter}\r\n")) {
        5
    } else {
        4
    };

    let search_area = &markdown[start_offset..];

    let closing_patterns = [
        format!("\n{delimiter}\n"),
        format!("\n{delimiter}\r\n"),
        format!("\r\n{delimiter}\n"),
        format!("\r\n{delimiter}\r\n"),
    ];

    let mut best_match: Option<(usize, usize)> = None;
    for pattern in &closing_patterns {
        if let Some(pos) = search_area.find(pattern) {
            if best_match.is_none() || pos < best_match.unwrap().0 {
                best_match = Some((pos, pattern.len()));
            }
        }
    }

    if let Some((pos, pattern_len)) = best_match {
        let raw = search_area[..pos].trim();
        let content_start = start_offset + pos + pattern_len;
        let body = &markdown[content_start..];
        StrippedFrontmatter {
            raw: Some(raw),
            body,
            format: Some(format),
            consumed: content_start,
        }
    } else {
        StrippedFrontmatter {
            raw: None,
            body: markdown,
            format: None,
            consumed: 0,
        }
    }
}

/// Parse frontmatter from a markdown document.
///
/// Returns the parsed frontmatter and the markdown body following it. If no
/// frontmatter delimiters are present, the body is the original input and the
/// frontmatter is `Frontmatter::default()`.
pub fn parse_frontmatter(markdown: &str) -> Result<(Frontmatter, &str), FrontmatterError> {
    let stripped = strip_frontmatter(markdown);
    let frontmatter = match (stripped.raw, stripped.format) {
        (Some(raw), Some(FrontmatterFormat::Toml)) => toml::from_str::<Frontmatter>(raw)
            .map_err(|e| FrontmatterError::Parse(format!("TOML: {e}")))?,
        (Some(raw), Some(FrontmatterFormat::Yaml)) => serde_yaml::from_str::<Frontmatter>(raw)
            .map_err(|e| FrontmatterError::Parse(format!("YAML: {e}")))?,
        _ => Frontmatter::default(),
    };
    Ok((frontmatter, stripped.body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_frontmatter() {
        let md = "---\ntitle: My Doc\nweight: 42\n---\n# Content";
        let (fm, body) = parse_frontmatter(md).unwrap();
        assert_eq!(fm.title, "My Doc");
        assert_eq!(fm.weight, 42);
        assert_eq!(body, "# Content");
    }

    #[test]
    fn toml_frontmatter() {
        let md = "+++\ntitle = \"My Doc\"\nweight = 42\n+++\n# Content";
        let (fm, body) = parse_frontmatter(md).unwrap();
        assert_eq!(fm.title, "My Doc");
        assert_eq!(fm.weight, 42);
        assert_eq!(body, "# Content");
    }

    #[test]
    fn no_frontmatter() {
        let md = "# Just Content";
        let (fm, body) = parse_frontmatter(md).unwrap();
        assert_eq!(fm, Frontmatter::default());
        assert_eq!(body, "# Just Content");
    }

    #[test]
    fn unterminated_frontmatter_is_body() {
        let md = "---\ntitle: foo\n# Body";
        let stripped = strip_frontmatter(md);
        assert!(stripped.raw.is_none());
        assert_eq!(stripped.body, md);
    }
}
