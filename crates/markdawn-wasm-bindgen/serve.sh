#!/usr/bin/env bash
# Build with wasm-pack, copy the pkg/ output next to index.html, then serve.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"

wasm-pack build "$here" --target web --release

cp "$here/pkg/markdawn_wasm_bindgen.js"      "$here/web/"
cp "$here/pkg/markdawn_wasm_bindgen_bg.wasm" "$here/web/"
cp "$here/pkg/markdawn_wasm_bindgen.d.ts"    "$here/web/"

# Lazily-loaded pikchr render module — its own wasm (~4 MB), imported by
# the frontend only when a `pikchr` block appears, kept out of the core
# module so it stays small. typst is intentionally excluded from the
# Preact build (too heavy at ~33 MB); typst blocks stay as code.
wasm-pack build "$repo_root/crates/markdawn-wasm-pikchr" --target web --release
cp "$repo_root/crates/markdawn-wasm-pikchr/pkg/markdawn_wasm_pikchr.js"      "$here/web/"
cp "$repo_root/crates/markdawn-wasm-pikchr/pkg/markdawn_wasm_pikchr_bg.wasm" "$here/web/"

# Inline examples/sample.md (HTML-escaped) into the textarea of each
# template. Single source of truth with the iced demo.
#   index.html — HTML-string target playground
#   vdom.html  — Preact (vdom) target playground
python3 - "$repo_root/examples/sample.md" "$here/web" <<'PY'
import html, os, sys
sample_path, web_dir = sys.argv[1:3]
with open(sample_path) as f:
    sample = html.escape(f.read())
for stem in ("index", "vdom"):
    with open(os.path.join(web_dir, f"{stem}.template.html")) as f:
        template = f.read()
    with open(os.path.join(web_dir, f"{stem}.html"), "w") as f:
        f.write(template.replace("{{SAMPLE}}", sample))
PY

echo "==> serving $here/web on http://localhost:8001/"
echo "      HTML target:   http://localhost:8001/index.html"
echo "      Preact target: http://localhost:8001/vdom.html"
cd "$here/web"
exec python3 -m http.server 8001
