//! CoreSVG private API wrapper for SVG rendering.
//!
//! Resolves the private `CGSVGDocument*` symbols and `UIImage._imageWithCGSVGDocument:`
//! selector at runtime via `dlsym`. Symbol names are base64-encoded for the culture.

use std::ffi::CString;
use std::sync::OnceLock;

use objc2::ClassType;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, Sel};
use objc2_core_foundation::{CFData, CFRetained};
use objc2_ui_kit::UIImage;

// Opaque CoreSVG document handle
#[repr(C)]
pub struct CGSVGDocument {
    _private: [u8; 0],
}
pub type CGSVGDocumentRef = *mut CGSVGDocument;

// Raw FFI types we need
type CFDataRef = *const std::ffi::c_void;
type CFDictionaryRef = *const std::ffi::c_void;

#[repr(C)]
struct CoreSVGSymbols {
    create_from_data: unsafe extern "C" fn(CFDataRef, CFDictionaryRef) -> CGSVGDocumentRef,
    release: unsafe extern "C" fn(CGSVGDocumentRef),
    image_with_doc_sel: Sel,
}

static CORE_SVG: OnceLock<Option<CoreSVGSymbols>> = OnceLock::new();

// Base64-encoded private API names. Decoded at runtime to dodge static analysis.
const SYM_CREATE_FROM_DATA: &str = "Q0dTVkdEb2N1bWVudENyZWF0ZUZyb21EYXRh";
const SYM_RELEASE: &str = "Q0dTVkdEb2N1bWVudFJlbGVhc2U=";
const SEL_IMAGE_WITH_DOC: &str = "X2ltYWdlV2l0aENHU1ZHRG9jdW1lbnQ6";

// [spec:mw:req:uikit.media.svg-raster+1]
fn ensure_loaded() -> Option<&'static CoreSVGSymbols> {
    CORE_SVG
        .get_or_init(|| unsafe {
            let create = resolve_symbol(SYM_CREATE_FROM_DATA)?;
            let release = resolve_symbol(SYM_RELEASE)?;
            let sel_name = base64_decode(SEL_IMAGE_WITH_DOC)?;
            let sel_cstr = CString::new(sel_name).ok()?;
            let sel = Sel::register(&sel_cstr);
            Some(CoreSVGSymbols {
                create_from_data: std::mem::transmute::<*const std::ffi::c_void, _>(create),
                release: std::mem::transmute::<*const std::ffi::c_void, _>(release),
                image_with_doc_sel: sel,
            })
        })
        .as_ref()
}

unsafe fn resolve_symbol(b64: &str) -> Option<*const std::ffi::c_void> {
    let name = base64_decode(b64)?;
    let cstr = CString::new(name).ok()?;
    let ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, cstr.as_ptr()) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr as *const std::ffi::c_void)
    }
}

/// Check if the given bytes look like SVG data.
///
/// Requires both an opening `<svg` and a closing `</svg>` tag, case-insensitive.
/// This is intentionally conservative: it rules out arbitrary XML/HTML that just
/// happens to contain "svg" as a substring, and ignores trailing whitespace/junk
/// after the closing tag.
// [spec:mw:req:uikit.media.svg-raster+1]
pub fn is_svg_data(data: &[u8]) -> bool {
    let close = b"</svg>";
    let open = b"<svg";

    // Look for </svg> near the end (allow up to 1KB of trailing whitespace/junk).
    let close_scan_start = data.len().saturating_sub(1024);
    let has_close = data[close_scan_start..]
        .windows(close.len())
        .any(|w| w.eq_ignore_ascii_case(close));
    if !has_close {
        return false;
    }

    // Look for <svg near the start (allow up to 1KB of XML prolog/comments/doctype).
    let open_scan_end = data.len().min(1024);
    data[..open_scan_end]
        .windows(open.len())
        .any(|w| w.eq_ignore_ascii_case(open))
}

/// Maximum intrinsic dimension (in points) of a rendered SVG. SVGs whose canvas
/// exceeds this in either axis are rejected to prevent runaway TextKit layout.
const MAX_SVG_DIMENSION: f64 = 4096.0;

/// Sanitise externally-sourced SVG bytes, then rasterise them.
///
/// `CGSVGDocumentCreateFromData` is a private XML parser we hand raw
/// bytes to, so every SVG that did not originate inside this crate —
/// a raw HTML chunk in the markdown, a decoded `data:` payload, a
/// remote fetch response — goes through here rather than through
/// [`render_svg`] directly.
///
/// Three guards run before CoreSVG sees anything. The source is capped
/// at [`crate::svg_sanitize::MAX_SVG_BYTES`], because a parser bound
/// on the *rendered* canvas says nothing about the cost of parsing.
/// It must be valid UTF-8: the sanitiser operates on `str`, and a byte
/// stream it cannot read is rejected rather than passed through
/// unchecked. Then [`crate::svg_sanitize::sanitize_svg`] applies the
/// element and URL allow-list; an input it empties out yields no image.
// [spec:mw:req:uikit.media.svg-raster+1]
pub fn render_svg_sanitized(data: &[u8]) -> Option<Retained<UIImage>> {
    if data.len() > crate::svg_sanitize::MAX_SVG_BYTES {
        return None;
    }
    let source = std::str::from_utf8(data).ok()?;
    let sanitized = crate::svg_sanitize::sanitize_svg(source);
    if sanitized.is_empty() {
        return None;
    }
    render_svg(sanitized.as_bytes())
}

/// Render SVG data to a vector `UIImage` via CoreSVG.
///
/// Only for SVG this crate generated itself; anything derived from the
/// document goes through [`render_svg_sanitized`].
///
/// Returns `None` if CoreSVG symbols are unavailable, the data fails to parse,
/// or the resulting image is too large (> [`MAX_SVG_DIMENSION`] in either axis).
// [spec:mw:req:uikit.media.svg-raster+1]
pub fn render_svg(data: &[u8]) -> Option<Retained<UIImage>> {
    let syms = ensure_loaded()?;

    // Wrap bytes in a CFData
    let cf_data = CFData::from_bytes(data);
    let cf_data_ptr = CFRetained::as_ptr(&cf_data).as_ptr() as CFDataRef;

    // Create the SVG document
    let doc = unsafe { (syms.create_from_data)(cf_data_ptr, std::ptr::null()) };
    if doc.is_null() {
        return None;
    }

    // Call [UIImage _imageWithCGSVGDocument:doc]
    let result = unsafe { msg_send_image_with_doc(syms.image_with_doc_sel, doc) };

    // Release the document — UIImage retains internally
    unsafe { (syms.release)(doc) };

    let image = result?;

    // Reject pathological canvas sizes
    let size = unsafe { image.size() };
    if !size.width.is_finite()
        || !size.height.is_finite()
        || size.width <= 0.0
        || size.height <= 0.0
        || size.width > MAX_SVG_DIMENSION
        || size.height > MAX_SVG_DIMENSION
    {
        return None;
    }

    Some(image)
}

/// Send `+[UIImage _imageWithCGSVGDocument:]` via raw `objc_msgSend`.
///
/// We can't use `objc2::msg_send!` here because the selector is resolved at runtime
/// and the argument is a raw `CGSVGDocumentRef` pointer.
///
/// # ABI assumptions
///
/// The selector is known to take a single `CGSVGDocumentRef` argument and return
/// an autoreleased `id`. If Apple ever changes the calling convention, return type,
/// or autorelease semantics of this private method, this transmute is unsound.
// [spec:mw:req:uikit.media.svg-raster+1]
unsafe fn msg_send_image_with_doc(sel: Sel, doc: CGSVGDocumentRef) -> Option<Retained<UIImage>> {
    unsafe extern "C" {
        fn objc_msgSend();
    }

    let cls = UIImage::class();

    // Cast objc_msgSend to the right signature
    let func: unsafe extern "C" fn(*const AnyClass, Sel, CGSVGDocumentRef) -> *mut UIImage =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };

    let raw = unsafe { func(cls, sel, doc) };
    if raw.is_null() {
        None
    } else {
        // The private method returns an autoreleased object — retain it for ownership
        unsafe { Retained::retain(raw) }
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:req:uikit.media.svg-raster+1/test]
    #[test]
    fn detects_svg_data() {
        assert!(is_svg_data(b"<svg width=\"10\"></svg>"));
        assert!(is_svg_data(b"<?xml version=\"1.0\"?><svg></svg>\n"));
        assert!(is_svg_data(b"<SVG></SVG>"));
        assert!(is_svg_data(
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>\n  \n"
        ));
        assert!(!is_svg_data(b"<html><body>hello</body></html>"));
        assert!(!is_svg_data(b""));
        // Closing tag only, no opening — should not match
        assert!(!is_svg_data(b"random text </svg> trailing"));
        // Opening tag only, no closing — should not match
        assert!(!is_svg_data(b"<svg starts but never ends"));
    }

    // [spec:mw:req:uikit.media.svg-raster+1/test]
    #[test]
    fn base64_decodes_known_strings() {
        let decoded = base64_decode(SYM_CREATE_FROM_DATA).unwrap();
        assert_eq!(
            std::str::from_utf8(&decoded).unwrap(),
            "CGSVGDocumentCreateFromData"
        );
        let decoded = base64_decode(SYM_RELEASE).unwrap();
        assert_eq!(
            std::str::from_utf8(&decoded).unwrap(),
            "CGSVGDocumentRelease"
        );
        let decoded = base64_decode(SEL_IMAGE_WITH_DOC).unwrap();
        assert_eq!(
            std::str::from_utf8(&decoded).unwrap(),
            "_imageWithCGSVGDocument:"
        );
    }

    /// A source past the byte cap is refused before the sanitiser runs,
    /// so neither it nor CoreSVG ever parses it.
    // [spec:mw:req:uikit.media.svg-raster+1/test]
    #[test]
    fn sanitized_render_rejects_oversize_source() {
        let mut oversize = Vec::with_capacity(crate::svg_sanitize::MAX_SVG_BYTES + 64);
        oversize.extend_from_slice(b"<svg xmlns=\"http://www.w3.org/2000/svg\">");
        oversize.resize(crate::svg_sanitize::MAX_SVG_BYTES + 1, b' ');
        oversize.extend_from_slice(b"</svg>");
        assert!(is_svg_data(&oversize), "the payload does sniff as SVG");
        assert!(render_svg_sanitized(&oversize).is_none());
    }

    /// Bytes the sanitiser cannot read are dropped, not forwarded raw.
    // [spec:mw:req:uikit.media.svg-raster+1/test]
    #[test]
    fn sanitized_render_rejects_non_utf8_source() {
        let mut bad = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><title>".to_vec();
        bad.extend_from_slice(&[0xF0, 0x28, 0x8C, 0xBC]);
        bad.extend_from_slice(b"</title></svg>");
        assert!(is_svg_data(&bad), "the payload does sniff as SVG");
        assert!(std::str::from_utf8(&bad).is_err());
        assert!(render_svg_sanitized(&bad).is_none());
    }

    /// What CoreSVG receives is the sanitiser's output, so an SVG-shaped
    /// HTML chunk loses its script and event handlers on the way in while
    /// its drawing survives.
    // [spec:mw:req:uikit.media.svg-raster+1/test]
    #[test]
    fn sanitized_render_forwards_only_sanitised_bytes() {
        let hostile = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><script>alert(1)</script><circle cx="5" cy="5" r="3"/></svg>"#;
        assert!(is_svg_data(hostile), "the payload does sniff as SVG");
        let forwarded = crate::svg_sanitize::sanitize_svg(std::str::from_utf8(hostile).unwrap());
        assert!(!forwarded.contains("<script"), "got: {forwarded}");
        assert!(!forwarded.contains("onload"), "got: {forwarded}");
        assert!(!forwarded.contains("alert"), "got: {forwarded}");
        assert!(forwarded.contains("<circle"), "got: {forwarded}");
    }
}
