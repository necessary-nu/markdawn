//! Defence-in-depth sanitiser for SVG blobs we emit into HTML.
//!
//! Every adapter path that produces or accepts attacker-influenced SVG
//! (graphviz output, pikchr output, future sources) runs its bytes
//! through [`sanitize_svg`] before they land in the rendered page via
//! `dangerous_inner_html` or a raw string concat.
//!
//! The actual sanitisation is delegated to the sibling
//! [`sanitizer`](../../../sanitizer-rs/) crate — a W3C Sanitizer API
//! implementation built on html5ever. It parses the SVG as HTML5
//! foreign content, walks the resulting DOM with the W3C safe-default
//! configuration's allow-list, drops `<script>` / `<animate>` /
//! `<foreignObject>`-style escape hatches, validates URL attributes
//! (including `xlink:href` and MathML navigational URLs) against a
//! `javascript:` / `vbscript:` reject-list, and serialises back out.
//!
//! We extend the W3C safe-default with one addition: `xlink:href` on
//! SVG `<a>`. Graphviz emits SVG 1.1 syntax which uses `xlink:href`
//! for links; the W3C safe-default only allows SVG 2's plain `href`,
//! so without this extension every graphviz-emitted link would be
//! stripped. URL-scheme validation still applies to the extended
//! attribute via sanitizer-rs's built-in `NAVIGATING_URL_ATTRIBUTES`
//! table (which does include `a@xlink:href` → javascript: check).
//!
//! We still keep [`MAX_SVG_BYTES`] here — sanitizer-rs doesn't bound
//! output size, and a successful parse-and-serialise of a 500 MB SVG
//! is still a 500 MB allocation we don't want to hand back to the
//! caller. Adapter call sites check the size cap before and after.

use std::sync::LazyLock;

use sanitizer::{SanitizerAttribute, SanitizerConfig};

/// Maximum byte length of a rendered SVG we'll pass back to the
/// caller. Graphviz can turn a small DOT input into a massive SVG
/// (long labels on many edges, etc.); cap the amplification.
// adv[impl output.size-cap]
pub const MAX_SVG_BYTES: usize = 5 * 1024 * 1024;

/// A cloned W3C safe-default config with `xlink:href` additionally
/// allowed on SVG `<a>`. Built once, reused across all sanitise calls.
static CONFIG: LazyLock<SanitizerConfig> = LazyLock::new(|| {
    let mut cfg = SanitizerConfig::safe_default();
    if let Some(elements) = cfg.elements.as_mut() {
        for el in elements.iter_mut() {
            let is_svg_a = el.name == "a"
                && el.namespace.as_deref() == Some("http://www.w3.org/2000/svg");
            if is_svg_a {
                if let Some(attrs) = el.attributes.as_mut() {
                    attrs.push(SanitizerAttribute::xlink("href"));
                }
            }
        }
    }
    cfg
});

/// Sanitise `svg`. Returns the sanitised string. Never fails; if the
/// underlying sanitiser rejects the input, we return an empty string
/// (callers treat empty output as "rendering failed, fall back to
/// plain code block").
// adv[impl output.url-allowlist]
// adv[impl output.no-script]
// adv[impl output.no-event-handlers]
// adv[impl output.no-external-entities]
// adv[impl output.no-stylesheet-pi]
// adv[impl output.inline-trim]
// adv[impl output.foreign-object-safe]
// adv[impl output.animate-href-safe]
pub fn sanitize_svg(svg: &str) -> String {
    match CONFIG.sanitize(svg) {
        Ok(out) => out.into_inner(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // adv[verify output.size-cap]
    #[test]
    fn size_cap_constant_bounds_expected() {
        assert!(MAX_SVG_BYTES >= 1024 * 1024);
    }

    // adv[verify output.no-stylesheet-pi]
    #[test]
    fn xml_stylesheet_pi_dropped() {
        let svg = r#"<?xml-stylesheet href="javascript:alert(1)"?><svg><rect/></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("xml-stylesheet"), "got: {out}");
        assert!(!out.contains("javascript:"), "got: {out}");
    }

    // adv[verify output.no-script]
    #[test]
    fn script_block_dropped() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script><circle/></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("<script"), "got: {out}");
        assert!(!out.contains("alert"), "got: {out}");
    }

    // adv[verify output.no-script]
    #[test]
    fn script_block_case_insensitive() {
        let svg = r#"<svg><ScRiPt>alert(1)</ScRiPt></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.to_ascii_lowercase().contains("<script"), "got: {out}");
    }

    // adv[verify output.no-event-handlers]
    #[test]
    fn event_handler_attribute_dropped() {
        let svg = r#"<svg><circle onclick="alert(1)" cx="5" cy="5" r="3"/></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("onclick"), "got: {out}");
        assert!(out.contains("<circle"), "circle structure preserved: {out}");
    }

    // adv[verify output.no-event-handlers]
    #[test]
    fn onload_handler_dropped_from_svg_root() {
        let svg = r#"<svg onload="alert(1)"><circle/></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("onload"), "got: {out}");
    }

    // adv[verify output.url-allowlist]
    #[test]
    fn javascript_xlink_href_is_neutered() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
            <a xlink:href="javascript:alert(1)"><rect width="10" height="10"/></a>
        </svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("javascript:"), "got: {out}");
        assert!(!out.contains("alert"), "got: {out}");
    }

    // adv[verify output.url-allowlist]
    #[test]
    fn javascript_href_plain_is_neutered() {
        let svg = r#"<svg><a href="javascript:alert(1)"><rect/></a></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("javascript:"), "got: {out}");
    }

    // adv[verify output.url-allowlist]
    #[test]
    fn href_with_fragment_survives() {
        let svg = r##"<svg><a href="#section"><rect/></a></svg>"##;
        let out = sanitize_svg(svg);
        assert!(out.contains("#section"), "benign fragment survived: {out}");
    }

    // adv[verify output.url-allowlist]
    #[test]
    fn href_with_relative_url_survives() {
        let svg = r#"<svg><a href="page.html"><rect/></a></svg>"#;
        let out = sanitize_svg(svg);
        assert!(out.contains("page.html"), "relative URL survived: {out}");
    }

    // adv[verify output.url-allowlist] — graphviz-shaped xlink:href.
    #[test]
    fn graphviz_shaped_safe_xlink_href_survives() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
            <g class="node"><a xlink:href="https://example.com/page"><ellipse/></a></g>
        </svg>"#;
        let out = sanitize_svg(svg);
        assert!(out.contains("https://example.com/page"), "got: {out}");
    }

    // adv[verify output.animate-href-safe]
    // <animate attributeName="href" values="javascript:..."> is a known SVG XSS
    // class. The W3C safe-default drops <animate> because it's not on the
    // allow-list. Neither of the attacker payloads survives.
    #[test]
    fn animate_element_dropped_entirely() {
        let svg = r#"<svg><a><rect/><animate attributeName="href" values="javascript:alert(1)"/></a></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("<animate"), "got: {out}");
        assert!(!out.contains("javascript:"), "got: {out}");
    }

    // adv[verify output.animate-href-safe]
    #[test]
    fn set_element_dropped_entirely() {
        let svg = r#"<svg><a><rect/><set attributeName="href" to="javascript:alert(1)"/></a></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("<set"), "got: {out}");
        assert!(!out.contains("javascript:"), "got: {out}");
    }

    // adv[verify output.foreign-object-safe]
    // foreignObject is on the SVG allow-list (useful for MathML / HTML-in-SVG)
    // but its contents go through the HTML sanitiser, so a <script> hosted
    // inside foreignObject's <body> is still dropped.
    #[test]
    fn foreign_object_cannot_host_script() {
        let svg = r#"<svg><foreignObject><body xmlns="http://www.w3.org/1999/xhtml">
            <script>alert(1)</script>
            <p>benign</p>
        </body></foreignObject></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("<script"), "got: {out}");
        assert!(!out.contains("alert"), "got: {out}");
    }

    // adv[verify output.no-external-entities]
    #[test]
    fn entity_declarations_dropped() {
        // DOCTYPE with internal subset. html5ever drops entity declarations
        // on parse; the entity never materialises in the output.
        let svg = r#"<!DOCTYPE svg [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
            <svg><text>&xxe;</text></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("<!ENTITY"), "got: {out}");
        assert!(!out.contains("/etc/passwd"), "got: {out}");
    }

    // adv[verify output.inline-trim]
    // html5ever's parse loses the XML prologue automatically because it's a
    // bogus comment in HTML parse context.
    #[test]
    fn xml_prologue_dropped() {
        let svg = r#"<?xml version="1.0"?><svg><rect/></svg>"#;
        let out = sanitize_svg(svg);
        assert!(!out.contains("<?xml"), "got: {out}");
        assert!(out.contains("<svg"), "svg still there: {out}");
    }

    #[test]
    fn benign_svg_round_trips_intact() {
        // Minimal graphviz-shaped SVG with real content comes back with
        // the graph structure intact. Attribute order may differ after
        // DOM round-trip, but the elements are all there.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80">
            <g id="graph0"><ellipse cx="50" cy="40" rx="20" ry="15"/></g>
        </svg>"#;
        let out = sanitize_svg(svg);
        assert!(out.contains("<svg"), "got: {out}");
        assert!(out.contains("<ellipse"), "got: {out}");
        assert!(out.contains("cx=\"50\"") || out.contains("cx='50'"), "got: {out}");
    }
}
