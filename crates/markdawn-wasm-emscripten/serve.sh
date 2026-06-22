#!/usr/bin/env bash
# Build the wasm bundle, copy it next to index.html, then serve.
# Run from anywhere; paths are absolute relative to this script.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
target_dir="$repo_root/target/wasm32-unknown-emscripten/release/deps"
web_dir="$here/web"

# Build each crate in its own `cargo build` invocation. Cargo unifies
# features across packages built together, so a single multi-package
# build would propagate `typst` / `pikchr` from the renderer crates'
# markdawn dep onto the main bundle's markdawn dep — re-bloating it
# with everything we just split out.
for pkg in markdawn-wasm-emscripten markdawn-render-graphviz markdawn-render-typst markdawn-render-pikchr; do
  cargo build --target wasm32-unknown-emscripten --release \
    -p "$pkg" \
    --manifest-path "$repo_root/Cargo.toml"
done

cp "$target_dir/markdawn_wasm_emscripten.js"   "$web_dir/"
cp "$target_dir/markdawn_wasm_emscripten.wasm" "$web_dir/"
cp "$target_dir/markdawn_render_graphviz.js"   "$web_dir/"
cp "$target_dir/markdawn_render_graphviz.wasm" "$web_dir/"
cp "$target_dir/markdawn_render_typst.js"      "$web_dir/"
cp "$target_dir/markdawn_render_typst.wasm"    "$web_dir/"
cp "$target_dir/markdawn_render_pikchr.js"     "$web_dir/"
cp "$target_dir/markdawn_render_pikchr.wasm"   "$web_dir/"

# Inline examples/sample.md (HTML-escaped) into the textarea of
# index.template.html. Single source of truth with the iced demo.
python3 - "$web_dir/index.template.html" "$repo_root/examples/sample.md" \
  "$web_dir/index.html" <<'PY'
import html, sys
template_path, sample_path, out_path = sys.argv[1:4]
with open(template_path) as f: template = f.read()
with open(sample_path) as f: sample = f.read()
with open(out_path, "w") as f:
    f.write(template.replace("{{SAMPLE}}", html.escape(sample)))
PY

echo "==> serving $web_dir on http://localhost:8000/"
cd "$web_dir"
exec python3 -m http.server 8000
