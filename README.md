# markdawn

A markdown parser with pluggable rendering adapters (HTML, Dioxus SSR,
ANSI/terminal, Typst, Iced, UIKit) and embedded diagram engines
(graphviz, pikchr, typst).

## Cargo features

The parser and HTML adapter are always compiled. Everything else is
opt-in:

| feature      | enables                                                    |
| ------------ | ---------------------------------------------------------- |
| `highlight`  | tree-sitter syntax highlighting via [ntreesitter]          |
| `dioxus`     | Dioxus SSR adapter                                         |
| `ansi`       | ANSI-to-HTML conversion                                    |
| `terminal`   | full terminal emulator (vt100), implies `ansi`             |
| `typst`      | Typst diagram engine + Typst output adapter                |
| `pikchr`     | pikchr diagram engine                                      |
| `graphviz`   | graphviz/DOT diagram engine (vendored, static)             |
| `iced`       | Iced GUI adapter (wgpu + tiny-skia)                        |
| `tracey`     | tracey spec-coverage annotations                           |

`highlight` on its own ships the highlighting machinery with no
languages linked, which is what the Swift and Android targets want —
they load grammars at runtime from plugin libraries. The default
`highlight-all-languages` adds every language in the ntreesitter
catalog.

[ntreesitter]: https://github.com/necessary-nu/ntreesitter

## Building

### Native

```sh
cargo build                              # parser + HTML only
cargo build --features highlight,typst   # mix and match
```

The `graphviz` feature builds graphviz from the vendored submodule via
CMake. Requires `cmake`, `bison`, and `flex` on PATH (`brew install
cmake bison flex` on macOS).

### Wasm

| feature       | `wasm32-unknown-unknown` | `wasm32-unknown-emscripten` |
| ------------- | ------------------------ | --------------------------- |
| parser + HTML | ✓                        | ✓                           |
| `highlight`   | ✓ (needs llvm@20 — see below) | ✓                      |
| `typst`       | ✓                        | ✓                           |
| `pikchr`      | ✓                        | ✓                           |
| `dioxus`      | ✓                        | ✓                           |
| `ansi`        | ✓                        | ✓                           |
| `terminal`    | ✓                        | ✓                           |
| `tracey`      | ✓                        | ✓                           |
| `graphviz`    | ✗ (needs libc)           | ✓                           |
| `iced`        | n/a (native GUI)         | n/a                         |

A note on threads: `typst`, `pikchr`, and `markdawn-graphviz` spawn a
worker thread on native targets to bound runtime via `Duration`
timeouts. On wasm (`cfg(target_family = "wasm")`) there is no thread —
the render runs inline on the calling thread with no timeout watchdog
(the host is single-threaded and already sandboxes runaway work). So
these engines render normally on `wasm32-unknown-unknown`; they do *not*
surface a `ThreadSpawn` error. On `wasm32-unknown-emscripten`, threads
also work with `-pthread` but the same inline path is used.

#### `wasm32-unknown-unknown`

The `highlight-all-languages` feature pulls in ntreesitter's per-language
crates, which generate and compile C parsers at build time. Apple's
bundled clang has no wasm32
target — install `llvm@20` (or any LLVM with WASM support) and point
`CC_wasm32_unknown_unknown` / `AR_wasm32_unknown_unknown` at it.

```sh
brew install llvm@20    # one-off

cargo build --target wasm32-unknown-unknown --no-default-features
cargo build --target wasm32-unknown-unknown \
  --no-default-features --features 'typst,pikchr'

CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm@20/bin/clang \
AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm@20/bin/llvm-ar \
cargo build --target wasm32-unknown-unknown \
  --no-default-features --features highlight
```

#### `wasm32-unknown-emscripten`

For `graphviz` we use emscripten, which provides a libc and a working
CMake toolchain. Pure-Rust features (`typst`, `pikchr`, `highlight`)
work here too, with no `CC_*` env vars needed — emscripten's `emcc`
handles all of it.

```sh
brew install emscripten
rustup target add wasm32-unknown-emscripten

cargo build --target wasm32-unknown-emscripten \
  --no-default-features --features 'graphviz,typst,pikchr,highlight'
```

The `markdawn-graphviz` build script auto-detects the emscripten target
and:

- runs the cmake configure/build via `emcmake cmake` / `emmake cmake`
- pre-feeds `MATH_LIB=m` because emscripten folds libm into libc and
  `find_library(m)` returns NOTFOUND
- builds graphviz with `WITH_ZLIB=OFF` (zlib only gates compressed
  output streams, which `gvRenderData("svg")` doesn't use)
- skips the `-fstack-clash-protection` / `-fcf-protection` hardening
  flags, which emcc rejects

The `fork()`/`pipe()`/`waitpid()` crash-isolation path is gated on
`cfg(any(target_os = "linux", target_os = "macos"))` in `lib.rs` and
on the corresponding `MARKDAWN_GV_NO_FORK` define from `build.rs` in
`wrapper.c`. Every other target (emscripten, wasi, windows, ios, …)
goes through the in-process render path. Wasm sandboxes already
provide the isolation the fork wrapper was buying.

## Workspace layout

```
markdawn/                     top-level crate (parser + adapters)
crates/
  markdawn-graphviz/          graphviz wrapper (vendored libgvc / libcgraph)
  markdawn-render-graphviz/   standalone wasm render shim, one per engine
  markdawn-render-pikchr/
  markdawn-render-typst/
  markdawn-wasm-bindgen/      browser bindings (wasm-bindgen)
  markdawn-wasm-emscripten/   browser bindings (emscripten)
  markdawn-wasm-pikchr/
  markdawn-swift/             staticlib + C header for the Swift/UIKit target
  markdawn-android/           JNI bindings for the Android target
  markdawn-swift-lang-*/      104 generated grammar plugin cdylibs
```

The `markdawn-swift-lang-*` crates are generated by
`crates/markdawn-swift/generate-plugins.sh` and are not tracked in the
repository; regenerating them needs an `ntreesitter` checkout beside this
one, though building does not.

Every dependency outside this workspace is a git dependency —
`ntreesitter`, `mathml-rs`, `sanitizer-rs`, `npikchr` and the `iced`
fork. No dependency resolves through a relative path out of the
repository, which is what makes markdawn usable as a git dependency
itself: a path escaping the checkout cannot be resolved by a consumer
that only has the checkout.
