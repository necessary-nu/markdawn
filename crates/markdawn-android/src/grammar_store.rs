//! Process-global language registry, populated at runtime by
//! `markdawn-swift-lang-*` plugin `.so`s.
//!
//! Ported verbatim from `crates/markdawn-swift/src/grammar_store.rs` — the
//! mechanism is platform-neutral. The core `.so` ships with zero grammars
//! baked in (it depends on `markdawn` with just the `highlight` feature, not
//! `highlight-all-languages`). Kotlin's `System.loadLibrary` pulls a plugin
//! into the process and the JNI `loadLanguage` entry point registers it,
//! resolving the plugin's provider symbol and validating its descriptor. The
//! `VdomPipeline` builds its highlighter from the global registry, so any
//! subsequently rendered code block whose language has been registered will
//! be highlighted; unregistered languages fall through to plain monospace.

use std::ffi::c_char;
use std::sync::{Arc, OnceLock};

use ntreesitter::{DynamicRegistry, RegistryBuilder};
use ntreesitter_plugin::{NtsProviderEntry, PluginLoadError, load_provider};

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
///   2 — the plugin's entry symbol is not loaded
///   3 — the descriptor failed validation or conflicts with a registered one
///
/// The plugin exports `nts_init_<name>` with `-` replaced by `_`. Because the
/// symbol is per-language rather than one shared across every plugin, a
/// process-global lookup resolves the right one; a single shared symbol would
/// resolve to whichever library loaded first regardless of the language
/// asked for. The direction is inverted from the previous ABI: nothing is
/// handed to the plugin, its descriptor is pulled and validated here.
///
/// Nothing borrowed from the descriptor is copied out — the registry holds
/// `&'static` data owned by the plugin image, which must stay resident.
/// Android never unloads a library pulled in by `System.loadLibrary`.
// [spec:mw:def:ffi.plugin.grammar-registration+1]
// [spec:mw:req:ffi.ownership.borrowed-input+1]
pub(crate) fn register_language(name: &str) -> u32 {
    // SAFETY: the resolver returns a symbol from the loaded process image,
    // and `load_provider` validates the descriptor behind it before anything
    // is registered.
    match unsafe { load_provider(name, resolve_global) } {
        Ok(provider) => match provider.register(global_registry()) {
            Ok(_) => 0,
            Err(_) => 3,
        },
        Err(PluginLoadError::MissingEntrySymbol { .. }) => 2,
        Err(_) => 3,
    }
}

/// Look up a NUL-terminated symbol in the loaded process image.
fn resolve_global(symbol: &[u8]) -> Option<NtsProviderEntry> {
    // SAFETY: `symbol` is NUL-terminated by `load_provider`, which builds it
    // from the ABI's own prefix.
    let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr() as *const c_char) };
    if address.is_null() {
        return None;
    }
    // SAFETY: the provider ABI fixes this entry point's signature, and the
    // symbol name was derived from that same ABI.
    Some(unsafe { std::mem::transmute::<*mut libc::c_void, NtsProviderEntry>(address) })
}
