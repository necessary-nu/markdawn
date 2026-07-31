//! Process-global language registry, populated at runtime by
//! `markdawn-swift-lang-*` plugin dylibs.
//!
//! The main framework ships with zero grammars baked in (it depends on
//! `markdawn` with just the `highlight` feature, not `highlight-all-languages`).
//! Swift `dlopen`s a plugin framework and then calls `mw_register_language`
//! with the language name; the symbol lookup and descriptor validation happen
//! here in Rust. The UIKit `BlockSink` builds its highlighter from the global
//! registry, so any subsequently rendered code block whose language has been
//! registered will be highlighted; unregistered languages fall through to
//! plain monospace.

use std::ffi::c_char;
use std::sync::{Arc, OnceLock};

use ntreesitter::{DynamicRegistry, RegistryBuilder};
use ntreesitter_plugin::{NtsProviderEntry, load_provider};

static REGISTRY: OnceLock<Arc<DynamicRegistry>> = OnceLock::new();

pub fn global_registry() -> &'static Arc<DynamicRegistry> {
    REGISTRY.get_or_init(|| {
        let empty = RegistryBuilder::new()
            .build()
            .expect("an empty registry has no aliases to collide");
        Arc::new(DynamicRegistry::new(empty))
    })
}

/// Resolve a plugin's provider entry point and register its grammar.
///
/// Return codes:
///   0 — success
///   1 — null pointer or non-UTF-8 name
///   2 — the plugin's entry symbol is not loaded
///   3 — the descriptor failed validation or conflicts with a registered one
///
/// The plugin exports `nts_init_<name>` with `-` replaced by `_`; because the
/// symbol is per-language rather than shared across every plugin, a
/// process-global lookup resolves the right one. The direction is inverted
/// from the old ABI: nothing is handed to the plugin, its descriptor is
/// pulled and validated here.
///
/// # Safety
/// `name` must point to `name_len` readable bytes. The plugin library
/// exporting the resolved symbol must stay loaded for as long as the registry
/// can be observed, which holds because plugin frameworks are never unloaded.
// [spec:mw:def:ffi.plugin.grammar-registration+1]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_register_language(name: *const c_char, name_len: usize) -> u32 {
    let Some(name) = (unsafe { slice_to_str(name, name_len) }) else {
        return 1;
    };
    if name.is_empty() {
        return 1;
    }

    // SAFETY: the resolver hands back a symbol from the process image, which
    // the caller contract keeps loaded; `load_provider` validates the
    // descriptor behind it before anything is registered.
    let provider = unsafe { load_provider(name, |symbol| resolve_global(symbol)) };
    match provider {
        Ok(provider) => match provider.register(global_registry()) {
            Ok(_) => 0,
            Err(_) => 3,
        },
        Err(ntreesitter_plugin::PluginLoadError::MissingEntrySymbol { .. }) => 2,
        Err(_) => 3,
    }
}

/// Look up a NUL-terminated symbol in the loaded process image.
///
/// `RTLD_DEFAULT` is sound here only because every provider exports a
/// distinct `nts_init_<name>`; a single shared entry symbol would resolve to
/// whichever library loaded first, no matter which language was asked for.
fn resolve_global(symbol: &[u8]) -> Option<NtsProviderEntry> {
    let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr() as *const c_char) };
    if address.is_null() {
        return None;
    }
    // SAFETY: the provider ABI fixes this entry point's signature, and the
    // symbol name is derived from that same ABI's prefix.
    Some(unsafe { std::mem::transmute::<*mut libc::c_void, NtsProviderEntry>(address) })
}

// A zero length is the empty string whatever the pointer holds; anything else
// must be valid UTF-8 or the caller maps `None` onto its own return code.
// [spec:mw:sem:ffi.encoding.utf8+1]
unsafe fn slice_to_str<'a>(ptr: *const c_char, len: usize) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
    std::str::from_utf8(bytes).ok()
}
