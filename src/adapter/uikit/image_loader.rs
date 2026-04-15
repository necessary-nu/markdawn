//! Async image loading for remote URLs using NSURLSession + NSCache.
//!
//! Standard iOS pattern: fetch on background queue, decode to UIImage,
//! dispatch back to main thread, swap the attachment's placeholder image
//! for the real one, invalidate TextKit layout for the attachment's range.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use block2::RcBlock;
use dispatch2::MainThreadBound;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSData, NSError, NSRange, NSString, NSURL, NSURLResponse, NSURLSession};
use objc2_ui_kit::{
    NSTextAttachment, NSTextContentStorage, NSTextElementProvider, NSTextStorageObserving, UIImage,
};

use super::svg;

struct PendingEntry {
    attachment: Retained<NSTextAttachment>,
    content_storage: Retained<NSTextContentStorage>,
}

type BoundEntry = MainThreadBound<PendingEntry>;

struct LoaderState {
    pending: Mutex<HashMap<String, Vec<BoundEntry>>>,
}

// NSCache requires Send+Sync bounds on key/value types that objc2's generics
// don't satisfy. We use a plain HashMap behind a Mutex instead — the access
// pattern (write on background callback, read on main) is always serialized
// through run_on_main anyway.
static IMAGE_CACHE: OnceLock<Mutex<HashMap<String, Retained<UIImage>>>> = OnceLock::new();

fn image_cache() -> &'static Mutex<HashMap<String, Retained<UIImage>>> {
    IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

static SHARED: OnceLock<LoaderState> = OnceLock::new();

fn shared() -> &'static LoaderState {
    SHARED.get_or_init(|| LoaderState {
        pending: Mutex::new(HashMap::new()),
    })
}

/// Attempt to load an image from a remote URL. If cached, applies it
/// synchronously. Otherwise kicks off an async fetch via NSURLSession.shared
/// and will apply the image + invalidate layout when the fetch completes.
///
/// Must be called from the main thread.
pub(crate) fn load(
    url: &str,
    attachment: &NSTextAttachment,
    content_storage: &NSTextContentStorage,
    mtm: MainThreadMarker,
) {
    // Cache hit → apply immediately
    if let Some(image) = cached_image(url) {
        apply_image(attachment, &image);
        invalidate_layout(content_storage);
        return;
    }

    // Stash this attachment as pending, wrapped in MainThreadBound for Send safety.
    let entry = PendingEntry {
        attachment: attachment.retain(),
        content_storage: content_storage.retain(),
    };
    let bound = MainThreadBound::new(entry, mtm);

    let mut pending = shared().pending.lock().unwrap();
    let entries = pending.entry(url.to_string()).or_default();
    let already_in_flight = !entries.is_empty();
    entries.push(bound);
    drop(pending);

    if !already_in_flight {
        start_fetch(url);
    }
}

/// Check if a URL is in the image cache (previously loaded).
pub(crate) fn cached_image(url: &str) -> Option<Retained<UIImage>> {
    image_cache().lock().ok()?.get(url).map(|img| img.clone())
}

fn start_fetch(url: &str) {
    let Some(ns_url) = NSURL::URLWithString(&NSString::from_str(url)) else {
        fail(url);
        return;
    };

    let session = NSURLSession::sharedSession();
    let url_owned = url.to_string();

    let block = RcBlock::new(
        move |data: *mut NSData, _response: *mut NSURLResponse, error: *mut NSError| {
            handle_completion(&url_owned, data, error);
        },
    );

    let task = unsafe { session.dataTaskWithURL_completionHandler(&ns_url, &block) };
    task.resume();
}

fn handle_completion(url: &str, data_ptr: *mut NSData, error_ptr: *mut NSError) {
    if !error_ptr.is_null() || data_ptr.is_null() {
        fail(url);
        return;
    }

    let data: &NSData = unsafe { &*data_ptr };

    // Try SVG first, otherwise UIImage
    let bytes = unsafe { data.as_bytes_unchecked() };
    let image = if svg::is_svg_data(bytes) {
        svg::render_svg(bytes)
    } else {
        UIImage::imageWithData(data)
    };

    let Some(image) = image else {
        fail(url);
        return;
    };

    // Cache it (cap at 100 entries to bound memory)
    if let Ok(mut cache) = image_cache().lock() {
        if cache.len() >= 100 {
            if let Some(oldest) = cache.keys().next().cloned() {
                cache.remove(&oldest);
            }
        }
        cache.insert(url.to_string(), image.clone());
    }

    // Drain pending and apply on main thread
    let entries: Vec<BoundEntry> = {
        let mut pending = shared().pending.lock().unwrap();
        pending.remove(url).unwrap_or_default()
    };

    if entries.is_empty() {
        return;
    }

    dispatch2::run_on_main(move |mtm| {
        for bound in entries {
            let entry = bound.get(mtm);
            apply_image(&entry.attachment, &image);
            invalidate_layout(&entry.content_storage);
        }
    });
}

fn fail(url: &str) {
    if let Ok(mut pending) = shared().pending.lock() {
        pending.remove(url);
    }
}

fn apply_image(attachment: &NSTextAttachment, image: &UIImage) {
    let size = unsafe { image.size() };
    let max_width: f64 = 600.0;
    let (w, h) = if size.width > max_width {
        let scale = max_width / size.width;
        (max_width, size.height * scale)
    } else {
        (size.width, size.height)
    };

    attachment.setImage(Some(image));
    attachment.setBounds(CGRect::new(
        CGPoint::ZERO,
        CGSize {
            width: w,
            height: h,
        },
    ));
}

fn invalidate_layout(content_storage: &NSTextContentStorage) {
    // Setting the image on an existing NSTextAttachment doesn't fire a text
    // storage change notification, so TextKit 2 never knows to re-render the
    // glyph.  Touch the backing storage's attributes to trigger the
    // processEditing → NSTextContentStorage → layout manager chain.
    if let Some(storage) = content_storage.textStorage() {
        let len = storage.length();
        if len > 0 {
            storage.beginEditing();
            let attrs =
                unsafe { storage.attributesAtIndex_effectiveRange(0, core::ptr::null_mut()) };
            unsafe { storage.setAttributes_range(Some(&attrs), NSRange::new(0, 1)) };
            storage.endEditing();
        }
    }
    if let Some(layout_manager) = content_storage.primaryTextLayoutManager() {
        let range = content_storage.documentRange();
        layout_manager.invalidateLayoutForRange(&range);
    }
}
