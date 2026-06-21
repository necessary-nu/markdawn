// Contract test: drives the real compiled wasm exports (parseToVdom /
// VdomParser / VdomDocument) and runs the output through a copy of
// preact.js's toVNode mapping, to prove the JS<->wasm data shape lines
// up. Run: deno run --allow-read vdom_contract_test.ts
import { parseToVdom, VdomDocument, VdomParser } from "./pkg-deno/markdawn_wasm_bindgen.js";

// --- exact copy of preact.js's mapper, with a recording `h` shim ---
const Fragment = Symbol("Fragment");
// deno-lint-ignore no-explicit-any
function h(tag: any, props: any, ...children: any[]) {
  return { tag, props, children };
}
// deno-lint-ignore no-explicit-any
function toVNode(n: any): any {
  if (n == null) return null;
  if (n.text != null) return n.text;
  const props = n.props ?? {};
  if (n.html != null) {
    return h(n.tag ?? "div", { ...props, dangerouslySetInnerHTML: { __html: n.html } });
  }
  if (n.tag == null) return h(Fragment, null, ...(n.children ?? []).map(toVNode));
  return h(n.tag, props, ...(n.children ?? []).map(toVNode));
}

// deno-lint-ignore no-explicit-any
function reduce(events: any[]) {
  const map = new Map();
  for (const ev of events) {
    if (ev.type === "upsert") map.set(ev.id, ev.vtree);
    else if (ev.type === "delete") map.delete(ev.id);
  }
  return [...map.values()];
}

function streamBlocks(text: string, chunk = 7) {
  const p = new VdomParser();
  const map = new Map();
  const drain = () => {
    for (const ev of p.takeEvents()) {
      if (ev.type === "upsert") map.set(ev.id, ev.vtree);
      else if (ev.type === "delete") map.delete(ev.id);
    }
  };
  for (let i = 0; i < text.length; i += chunk) {
    p.pushStr(text.slice(i, i + chunk));
    drain();
  }
  p.finish();
  drain();
  return [...map.values()];
}

let pass = 0;
// deno-lint-ignore no-explicit-any
function check(name: string, fn: () => void) {
  fn();
  pass++;
  console.log("  ✓", name);
}
// deno-lint-ignore no-explicit-any
function eq(a: any, b: any, msg: string) {
  const A = JSON.stringify(a), B = JSON.stringify(b);
  if (A !== B) throw new Error(`${msg}\n  got:  ${A}\n  want: ${B}`);
}

check("heading shape + props is a PLAIN OBJECT (not a Map)", () => {
  const trees = parseToVdom("# Hi\n");
  console.log("    parseToVdom('# Hi') =>", JSON.stringify(trees));
  const h1 = trees[0];
  if (h1.props instanceof Map) throw new Error("props came across as a Map");
  if (Object.getPrototypeOf(h1.props) !== Object.prototype) throw new Error("props not a plain object");
  eq(trees, [{ tag: "h1", props: {}, children: [{ text: "Hi" }] }], "heading tree");
});

check("link url-safety + task-item boolean props", () => {
  const a = parseToVdom("[x](javascript:alert(1))\n")[0].children[0];
  if (a.props.href !== "") throw new Error("javascript: href not emptied");
  const input = parseToVdom("- [x] done\n")[0].children[0].children[0];
  if (input.props.disabled !== true) throw new Error("disabled not boolean true");
  if (input.props.checked !== true) throw new Error("checked not boolean true");
});

check("inline math is a raw <svg> node with class", () => {
  const span = parseToVdom("$x$\n")[0].children[0];
  if (span.tag !== "span" || typeof span.html !== "string" || !span.html.includes("<svg")) {
    throw new Error("math node not a raw svg span");
  }
  if (!String(span.props.class).includes("math")) throw new Error("math class missing");
});

check("streaming reconciles to the SAME tree as one-shot parse", () => {
  const doc =
    "# Title\n\nA paragraph with **bold** and a [link](https://x.com).\n\n" +
    "- one\n- two\n\n```rust\nfn main() {}\n```\n\n> quote\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
  eq(streamBlocks(doc), parseToVdom(doc), "streamed vs static");
});

check("VdomDocument.setSource + edit apply a keyed delta", () => {
  const doc = "# A\n\npara\n\n- x\n";
  const d = new VdomDocument();
  d.setSource(doc);
  eq(reduce(d.takeEvents()), parseToVdom(doc), "doc vs static");

  const store = new Map();
  const d2 = new VdomDocument();
  d2.setSource(doc);
  for (const ev of d2.takeEvents()) if (ev.type === "upsert") store.set(ev.id, ev.vtree);
  d2.edit(0, 3, "# B"); // "# A" -> "# B"
  for (const ev of d2.takeEvents()) {
    if (ev.type === "upsert") store.set(ev.id, ev.vtree);
    else if (ev.type === "delete") store.delete(ev.id);
  }
  const headingText = [...store.values()][0].children[0].text;
  if (headingText !== "B") throw new Error(`edit not applied, heading="${headingText}"`);
});

check("rawHtml option: safe sanitizes, unrestricted passes through", () => {
  const src = "<!DOCTYPE html>\n\nok\n";
  const raw = parseToVdom(src, { rawHtml: "unrestricted" })[0];
  if (raw.html == null || !raw.html.includes("<!DOCTYPE html>") || raw.html.includes("<html>")) {
    throw new Error("unrestricted did not pass doctype through: " + JSON.stringify(raw));
  }
  const safe = parseToVdom(src)[0]; // default = safe
  if (safe.html == null || !safe.html.includes("<html>")) {
    throw new Error("safe() did not sanitize doctype: " + JSON.stringify(safe));
  }
});

check("pikchr + typst both stay code blocks in core", () => {
  // Neither engine is in core. pikchr arrives as
  // <pre><code class="language-pikchr"> for the frontend to swap in SVG
  // from its lazy module; typst is excluded from this build entirely, so
  // it just stays a code block. (math, which IS baked, stays inline svg.)
  for (const lang of ["pikchr", "typst"]) {
    const node = parseToVdom(["```" + lang, "x", "```", ""].join("\n"))[0];
    if (node.tag !== "pre") {
      throw new Error(`${lang} should be a code block: ` + JSON.stringify(node).slice(0, 120));
    }
    const cls = node.children?.[0]?.props?.class ?? "";
    if (!String(cls).includes("language-" + lang)) {
      throw new Error(`${lang} code-block class wrong: ` + cls);
    }
  }
});

check("toVNode maps the full sample.md without throwing", () => {
  const sample = Deno.readTextFileSync(new URL("../../examples/sample.md", import.meta.url));
  const trees = parseToVdom(sample);
  let nodes = 0;
  // deno-lint-ignore no-explicit-any
  const count = (v: any) => {
    if (v == null || typeof v === "string") return;
    nodes++;
    (v.children ?? []).forEach(count);
  };
  trees.map(toVNode).forEach(count);
  if (nodes < 50) throw new Error(`only ${nodes} vnodes`);
  console.log(`    ${trees.length} blocks -> ${nodes} preact vnodes`);
});

console.log(`\n✅ ${pass}/${pass} contract checks passed`);
