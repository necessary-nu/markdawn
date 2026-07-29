//! Heading extraction and slug generation.

/// A heading extracted from the markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Heading text (without the `#` prefix).
    pub title: String,
    /// Slug id for linking (e.g. `my-heading`).
    pub id: String,
    /// Heading level (1-6).
    pub level: u8,
    /// 1-indexed line number.
    pub line: usize,
}

/// Generate a URL-safe slug from text.
///
/// Lowercases ASCII, keeps `[a-z0-9.]`, replaces other runs with single hyphens,
/// and trims leading/trailing hyphens.
// [spec:mw:req:traceyext.heading.slug]
pub fn slugify(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_hyphen = true;

    for c in text.chars() {
        if c.is_alphanumeric() || c == '.' {
            result.push(c.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            result.push('-');
            last_was_hyphen = true;
        }
    }

    if result.ends_with('-') {
        result.pop();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    // [spec:mw:req:traceyext.heading.slug/test]
    #[test]
    fn special_chars() {
        assert_eq!(slugify("My API (v2)"), "my-api-v2");
    }

    // [spec:mw:req:traceyext.heading.slug/test]
    #[test]
    fn dots_preserved() {
        assert_eq!(slugify("i.have.dots"), "i.have.dots");
    }

    #[test]
    fn collapse_spaces() {
        assert_eq!(slugify("a   b"), "a-b");
    }

    // [spec:mw:req:traceyext.heading.slug/test]
    #[test]
    fn trim_edges() {
        assert_eq!(slugify("  Hello  "), "hello");
    }
}
