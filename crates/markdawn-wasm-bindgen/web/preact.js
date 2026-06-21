// Thin Preact binding for markdawn's vdom target.
//
// The wasm side (`parseToVdom`, `VdomParser`, `VdomDocument`) hands JS a
// framework-neutral VNode tree, shaped as field-discriminated plain
// objects:
//
//   { text: "hi" }                              -> a text node
//   { tag, props, children: [...] }             -> an element
//   { tag, props, html }                        -> element w/ raw innerHTML
//   { children: [...] }   (no tag)              -> a fragment
//
// `html` payloads are already sanitized (diagram SVG, math SVG,
// highlighted code, filtered raw HTML) — they map to Preact's
// `dangerouslySetInnerHTML`.

import { h, render, Fragment } from "https://esm.sh/preact@10";
import { useState, useEffect } from "https://esm.sh/preact@10/hooks";

// Engines the core wasm leaves as plain code blocks, rendered via a
// separately-loaded wasm module the first time that block appears.
// Only pikchr (~4 MB) has a module. typst (~33 MB) is intentionally
// excluded from this build, and graphviz needs emscripten — both stay
// as plain code blocks.
const DIAGRAM_MODULES = {
  pikchr: "./markdawn_wasm_pikchr.js",
};

const engineCache = {};
function loadEngine(lang) {
  const url = DIAGRAM_MODULES[lang];
  if (!url) return Promise.resolve(null);
  if (!engineCache[url]) {
    engineCache[url] = import(url).then(async (mod) => {
      await mod.default(); // wasm-bindgen `--target web` init
      return mod;
    });
  }
  return engineCache[url];
}

/** Is this VNode a `<pre><code class="language-X">` for a lazy engine? */
function diagramOf(n) {
  if (n == null || n.tag !== "pre" || !Array.isArray(n.children) || n.children.length !== 1) return null;
  const code = n.children[0];
  if (!code || code.tag !== "code") return null;
  const m = /(?:^|\s)language-([^\s"]+)/.exec(code.props?.class ?? "");
  if (!m || !(m[1] in DIAGRAM_MODULES)) return null;
  const source = (code.children ?? []).map((c) => (c && c.text != null ? c.text : "")).join("");
  return { lang: m[1], source };
}

/** Lazily load the engine, render the source to SVG, fall back to the code. */
function DiagramBlock({ lang, source }) {
  const [state, setState] = useState({ status: "loading" });
  useEffect(() => {
    let alive = true;
    setState({ status: "loading" });
    loadEngine(lang)
      .then((mod) => {
        if (!alive) return;
        if (!mod) return setState({ status: "unsupported" });
        const svg = mod.render(source);
        setState(svg ? { status: "ok", svg } : { status: "error" });
      })
      .catch((e) => alive && setState({ status: "error", msg: String(e) }));
    return () => { alive = false; };
  }, [lang, source]);

  if (state.status === "ok") {
    return h("div", { class: `diagram diagram-${lang}`, dangerouslySetInnerHTML: { __html: state.svg } });
  }
  const note = state.status === "loading" ? `loading ${lang}…\n\n` : "";
  return h("pre", null, h("code", { class: `language-${lang}` }, note + source));
}

/** Map one markdawn VNode (plain object) to a Preact vnode. */
export function toVNode(n) {
  if (n == null) return null;
  if (n.text != null) return n.text;
  const diagram = diagramOf(n);
  if (diagram) return h(DiagramBlock, diagram);
  const props = n.props ?? {};
  if (n.html != null) {
    return h(n.tag ?? "div", { ...props, dangerouslySetInnerHTML: { __html: n.html } });
  }
  // A node without a tag (but with children) is a fragment.
  if (n.tag == null) {
    return h(Fragment, null, ...(n.children ?? []).map(toVNode));
  }
  return h(n.tag, props, ...(n.children ?? []).map(toVNode));
}

/** Host component for one top-level block; re-renders when its vtree changes. */
function Block({ vtree }) {
  return toVNode(vtree);
}

/**
 * Render a static markdown string into `el`.
 * @param {(source: string, options?: object) => any[]} parseToVdom  the wasm `parseToVdom`
 * @param {object} [options]  e.g. `{ rawHtml: "unrestricted" }`
 */
export function renderMarkdown(parseToVdom, source, el, options) {
  const trees = parseToVdom(source, options);
  render(h(Fragment, null, ...trees.map(toVNode)), el);
}

/**
 * An ordered, keyed store of top-level blocks. Apply upsert/delete
 * events from `VdomParser`/`VdomDocument`, then `renderTo(el)` — Preact
 * reconciles per block by key, so only changed blocks touch the DOM.
 */
export class BlockStore {
  constructor() {
    /** @type {Map<number, any>} insertion-ordered id -> vtree */
    this.blocks = new Map();
  }

  /** Apply a `takeEvents()` batch. */
  apply(events) {
    for (const ev of events) {
      if (ev.type === "upsert") this.blocks.set(ev.id, ev.vtree);
      else if (ev.type === "delete") this.blocks.delete(ev.id);
    }
  }

  renderTo(el) {
    const children = [...this.blocks].map(([id, vtree]) => h(Block, { key: id, vtree }));
    render(h(Fragment, null, ...children), el);
  }
}
