# FFI

The foreign-function bindings that expose markdawn to hosts that cannot link
Rust directly: a C-ABI staticlib for Swift/iOS, a JNI `cdylib` for
Android/Kotlin, a `wasm-bindgen` `cdylib` and an emscripten C-ABI module for
the browser, plus the dynamically-loaded grammar-plugin and diagram-render
modules those two wasm targets pull in on demand.

Getting the ownership half of this wrong is a use-after-free or a leak inside a
host application, so the transfer rules are stated per pointer kind rather than
per entry point.

## Handles and ownership

> [spec:mw:def:ffi.ownership.handle]
> An FFI *handle* is an opaque, owning reference to Rust-allocated parser state
> that lives across calls. The C ABI mints handles from `Box::into_raw`:
> `mw_state_new() -> *mut State` (declared as `MwState*` in
> `crates/markdawn-swift/include/markdawn.h`) and
> `mw_uikit_state_new() -> *mut UIKitState` (`MwUIKitState*`, present only on
> iOS/Catalyst and gated in the header on `__has_include(<UIKit/UIKit.h>)`).
> The JNI surface mints the same thing as a `jlong` from `parserNew` and
> `documentNew`. A host stores a handle opaquely: it never dereferences the
> pointee, never copies the pointee, and never constructs a handle itself. A
> handle is valid from the moment its constructor returns a non-null value
> until its matching destructor is called, and is dangling from then on.

> [spec:mw:req:ffi.ownership.handle-release]
> Rust allocates every handle and Rust MUST be the side that frees it. Each
> handle MUST be released exactly once, through the destructor paired with the
> constructor that produced it: `mw_state_new`/`mw_state_free`,
> `mw_uikit_state_new`/`mw_uikit_state_free`, `parserNew`/`parserFree`,
> `documentNew`/`documentFree`. A host MUST NOT release a handle with the C
> library's `free`, with an emscripten `Module._free`, or with a destructor
> belonging to a different constructor — the allocation carries Rust layout
> metadata that only the matching `Box::from_raw` can honour. Destructors
> accept a null pointer (or a zero `jlong`) and return without doing anything,
> so a host may call them unconditionally on a nullable field; every other
> pointer value is either a live handle or undefined behaviour. A handle MUST
> NOT be used after its destructor returns.

> [spec:mw:req:ffi.ownership.string-free]
> Every `*mut c_char` returned across the C ABI is a `CString::into_raw`
> allocation whose ownership transfers to the caller, and the caller MUST
> return it to `mw_string_free` — never to `free`, to `Module._free`, or to any
> other deallocator. Exactly four functions return such a pointer:
> `mw_state_render`, `mw_render_dot`, `mw_render_typst` and `mw_render_pikchr`.
> `mw_string_free` treats null as a no-op, so the null that a failing render
> returns may be passed through unchecked. Because each lazily-loaded
> emscripten module links its own allocator over its own linear memory, a
> pointer MUST be released by the `mw_string_free` exported by the same module
> that produced it; the copies exported by the core module and by each
> `markdawn-render-*` module are not interchangeable, and crossing them frees
> an address in the wrong heap.

> [spec:mw:req:ffi.ownership.borrowed-input]
> Pointers passed *into* the ABI are borrowed for the duration of the call and
> no longer. `mw_state_push_chunk`, `mw_uikit_state_push_chunk`,
> `mw_render_dot`, `mw_render_typst`, `mw_render_pikchr` and every
> `(ptr, len)` pair of `mw_register_grammar` MUST reference memory readable for
> the length given, and the callee MUST copy anything it needs to retain before
> returning. The host keeps ownership and MAY free or reuse the buffer as soon
> as the call returns. The one argument that outlives its call is
> `mw_register_grammar`'s `lang_fn`: the compiled grammar holds static data
> reachable through that function pointer, so the plugin image it came from
> MUST stay loaded for the remaining life of the process.

> [spec:mw:req:ffi.ownership.retained-objc]
> `mw_uikit_state_stack_view` and `mw_uikit_state_attributed_text` return
> Objective-C objects at +1: ownership of one retain count transfers to the
> caller, which MUST balance it — `Unmanaged.fromOpaque(_:).takeRetainedValue()`
> in Swift, `release`/`CFRelease` in C. The `BlockSink` behind the handle holds
> its own separate reference to the stack view, so the returned view stays
> valid and keeps updating in place while the host pushes further chunks into
> the same handle; releasing the caller's reference does not tear down the
> sink's view. Both functions return null when the handle is null, and null
> MUST NOT be released.

## Boundary safety

> [spec:mw:req:ffi.safety.null-guard]
> Every C-ABI entry point MUST check its handle argument for null before
> dereferencing and report the failure in-band instead of trapping: functions
> returning `u32` return `0` on success and non-zero on failure (`1` for a
> rejected argument on the state ABI; `mw_register_grammar` uses a wider code
> range), functions returning a pointer return null, and destructors return
> silently. A constructor that cannot honour its precondition reports it the
> same way — `mw_uikit_state_new` returns null rather than aborting when it is
> called off the main thread. `mw_state_push_chunk` and
> `mw_uikit_state_push_chunk` reject a null data pointer only when `len > 0`; a
> null pointer with `len == 0` is accepted as an empty chunk. The JNI surface
> does NOT extend this guarantee to handles: only `parserFree` and
> `documentFree` test their `jlong` for zero, so passing a zero or already-freed
> handle to any other `Native` method dereferences it. Nothing on either
> surface can detect a stale-but-non-null handle.

> [spec:mw:req:ffi.safety.no-unwind]
> A Rust panic MUST NOT unwind into a host frame. The release profile is built
> with `panic = "abort"`, so a panic terminates the process rather than crossing
> the boundary; entry points are not wrapped in `catch_unwind` and MUST NOT be
> written to depend on one. Every *expected* failure is therefore converted to
> an in-band result before the function returns: a `u32` status code on the C
> ABI, a null pointer where the return type is a pointer, a thrown
> `java.lang.RuntimeException` plus a null return on the JNI surface, and a
> `JsError` on the wasm-bindgen surface. Fallible host-side conversions follow
> the same discipline — a JNI string that will not convert yields the empty
> string rather than an error path that could panic.

> [spec:mw:sem:ffi.encoding.utf8]
> Text crossing the boundary is UTF-8. The byte-oriented entry points
> (`mw_state_push_byte`, `mw_state_push_chunk`, `mw_uikit_state_push_chunk`)
> tolerate a chunk that splits a multi-byte code point: incoming bytes are
> appended to a pending buffer, the longest valid UTF-8 prefix is forwarded to
> the parser and drained from the buffer, and any trailing incomplete sequence
> stays buffered for the next call. A byte that cannot begin a valid sequence
> therefore stalls the stream rather than erroring — the valid prefix stays
> length zero and later pushes accumulate behind it unparsed — until
> `mw_state_finish`, which discards the unparseable remainder and flushes what
> the parser already has. Ill-formed input is dropped, never reported. Strings
> handed back to the host are NUL-terminated UTF-8, so a render whose output
> contains an interior NUL yields null instead of a silently truncated string.
> `mw_register_grammar` validates each of its `(ptr, len)` arguments as UTF-8
> and reports which one failed through its return code, treating a zero length
> as the empty string regardless of the pointer.

## Host surfaces

> [spec:mw:def:ffi.host.jni]
> The Android surface is a `cdylib` bound to the Kotlin object
> `nu.necessary.markdawn.Native`. Every entry point is a
> `#[unsafe(no_mangle)] pub extern "system" fn` named
> `Java_nu_necessary_markdawn_Native_<method>` whose first two parameters are
> `JNIEnv` and `JClass`; the symbol name *is* the binding, so renaming the
> Kotlin package, class or method breaks the link at first call rather than at
> build time. Handles cross as `jlong`; every other value crosses as a
> `jstring` carrying JSON, because Kotlin deserializes with
> kotlinx.serialization rather than sharing a value representation. Inbound
> options are a JSON object (`{"rawHtml":"unrestricted"}`; an absent, unknown or
> malformed value falls back to the sanitizing default). Outbound payloads are
> either a `VNode[]` array (`parseToVdom`) or an event array of
> `{"type":"upsert",id,vtree}` and `{"type":"delete",id}` objects
> (`parserTakeEvents`, `documentTakeEvents`). `documentSetCursor` encodes "no
> cursor" as a negative `byteOffset`, since `jlong` has no null.

> [spec:mw:def:ffi.host.wasm-bindgen]
> The browser surface is a `#[wasm_bindgen]` `cdylib` that exposes markdawn as
> JS classes and free functions instead of raw pointers: `parseToHtml`,
> `serializeMarkdown`, `parseMarkdawnUrl` and `parseToVdom`, plus the classes
> `ParserBuilder`, `Parser`, `EditableDocument`, `VdomParser` and
> `VdomDocument`. Rust snake_case names are renamed to JS camelCase via
> `js_name`; generic types are monomorphized at `Ext = ()`; trait-shaped APIs
> (`Sink`, `Extension`) are not expressible to JS and appear only indirectly, as
> the concrete owning wrapper classes. Ownership is managed by the generated JS
> glue — each class instance owns a boxed Rust value released by the wrapper's
> `free()` or `[Symbol.dispose]()` — so none of the `mw_string_free` /
> `mw_*_free` discipline applies on this surface. `Result<_, JsValue>` surfaces
> as a thrown JS `Error` and `Option<T>` as `T | undefined`.

## Dynamically loaded modules

> [spec:mw:def:ffi.plugin.grammar-registration]
> Tree-sitter grammars are not linked into the Swift framework or the Android
> `.so`. Both ship the highlighting machinery over an initially empty
> process-global `GrammarStore` and acquire grammars at runtime from plugin
> libraries. A plugin exports exactly one symbol,
> `mw_lang_<sanitized>_init(register: RegisterFn) -> u32`, where `<sanitized>`
> is the language name with `-` replaced by `_` (`c-sharp` becomes
> `mw_lang_c_sharp_init`) and `RegisterFn` is the signature of
> `mw_register_grammar`. The host loader resolves that symbol — Swift `dlopen`s
> `MarkdawnLang<Pascal>.framework` and `dlsym`s it, Android `dlsym`s
> `RTLD_DEFAULT` after Kotlin's `System.loadLibrary` has pulled the plugin in —
> and calls it with the address of `mw_register_grammar`. That handoff is the
> only way a plugin obtains the registration function; it does not link against
> the core library. The plugin calls back with the language's *unsanitized*
> name (`c-sharp`, matching markdown info strings), its `LanguageFn` erased to
> `*const c_void`, and its three query sources. `mw_register_grammar` compiles
> them and inserts the result into the global store, returning `0` on success,
> `1` for a null pointer or non-UTF-8 name, `2`, `3` or `4` for a non-UTF-8
> highlights, injections or locals query respectively, and `5` when compilation
> fails. Registration is additive and takes effect for subsequently rendered
> blocks: a code block whose info string names a registered language is
> highlighted, one that does not falls through to plain monospace. The plugin
> crates are mechanically generated, one per arborium language, by
> `crates/markdawn-swift/generate-plugins.sh` and are not tracked in the
> repository.

> [spec:mw:sem:ffi.render-shim.lazy-module]
> Diagram engines are not bundled into the core wasm module. Each is built as
> its own emscripten module from a `markdawn-render-<engine>` crate exporting
> `mw_render_<engine>(ptr: *const u8, len: usize) -> *mut c_char` alongside
> `mw_string_free`, `malloc` and `free`, under a distinct
> `EXPORT_NAME=createMarkdawn<Engine>` so several modules can coexist on one
> page. The core render leaves a diagram block as
> `<pre><code class="language-<lang>">`; the frontend scans each freshly
> rendered preview for those blocks, marks every match before starting async
> work so a re-entrant render pass cannot double-process it, `import()`s the
> matching module on first encounter, and caches the instantiated module for
> the life of the page. To render, the host copies the source into *that*
> module's heap with `_malloc` plus `HEAPU8.set`, calls `mw_render_<engine>`,
> frees the input, and — only when the result is non-null — reads it with
> `UTF8ToString` and releases it with the same module's `mw_string_free`. A
> null result means the render failed and the caller leaves the original code
> block in place. The wasm-bindgen frontend uses the same lazy-module split but
> reaches it through a `wasm-bindgen` entry point (`markdawn-wasm-pikchr`'s
> `render`) rather than the C ABI, signalling the same failure as `undefined`.
