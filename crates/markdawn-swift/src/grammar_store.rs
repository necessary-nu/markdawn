//! Process-global tree-sitter grammar store, populated at runtime by
//! `markdawn-swift-lang-*` plugin cdylibs.
//!
//! The main framework ships with zero grammars baked in (it depends on
//! `markdawn` with just the `highlight` feature, not `highlight-all-languages`).
//! Plugins call `mw_register_grammar` via a function pointer the Swift loader
//! hands them. The UIKit `BlockSink` constructs its `Highlighter` from the
//! global store, so any subsequently rendered code block whose language has
//! been registered will be highlighted; unregistered languages fall through
//! to plain monospace.

use std::ffi::{c_char, c_void};
use std::sync::{Arc, OnceLock};

use markdawn::arborium::GrammarStore;
use markdawn::arborium::advanced::{CompiledGrammar, GrammarConfig};
use tree_sitter_language::LanguageFn;

static STORE: OnceLock<Arc<GrammarStore>> = OnceLock::new();

pub fn global_store() -> &'static Arc<GrammarStore> {
    STORE.get_or_init(|| Arc::new(GrammarStore::new()))
}

/// Register a tree-sitter grammar under `name`. Called by plugin cdylibs via
/// the function-pointer handoff orchestrated by the Swift loader.
///
/// Return codes:
///   0 — success
///   1 — null pointer or non-UTF-8 name
///   2 — non-UTF-8 highlights query
///   3 — non-UTF-8 injections query
///   4 — non-UTF-8 locals query
///   5 — grammar compilation failed (bad query or language mismatch)
///
/// # Safety
/// All pointers must point to valid, readable byte slices of the indicated
/// lengths. `lang_fn` must be the raw function pointer obtained from
/// `LanguageFn::into_raw` (i.e. `unsafe extern "C" fn() -> *const ()`),
/// erased to `*const c_void`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mw_register_grammar(
    name: *const c_char,
    name_len: usize,
    lang_fn: *const c_void,
    highlights: *const c_char,
    highlights_len: usize,
    injections: *const c_char,
    injections_len: usize,
    locals: *const c_char,
    locals_len: usize,
) -> u32 {
    if name.is_null() || lang_fn.is_null() {
        return 1;
    }
    let name = match unsafe { slice_to_str(name, name_len) } {
        Some(s) => s.to_owned(),
        None => return 1,
    };
    let highlights = match unsafe { slice_to_str(highlights, highlights_len) } {
        Some(s) => s,
        None => return 2,
    };
    let injections = match unsafe { slice_to_str(injections, injections_len) } {
        Some(s) => s,
        None => return 3,
    };
    let locals = match unsafe { slice_to_str(locals, locals_len) } {
        Some(s) => s,
        None => return 4,
    };

    let raw_fn: unsafe extern "C" fn() -> *const () = unsafe { std::mem::transmute(lang_fn) };
    let lang_fn = unsafe { LanguageFn::from_raw(raw_fn) };

    let config = GrammarConfig {
        language: lang_fn.into(),
        highlights_query: highlights,
        injections_query: injections,
        locals_query: locals,
    };
    let grammar = match CompiledGrammar::new(config) {
        Ok(g) => g,
        Err(_) => return 5,
    };
    global_store().insert(name, Arc::new(grammar));
    0
}

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
