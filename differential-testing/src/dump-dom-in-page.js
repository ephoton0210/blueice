// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A JS port of `blueice_dom::dump`'s exact algorithm and output format
// (`backend/core/dom/src/lib.rs`), run inside a live Chromium tab via
// `page.evaluate()` -- so the DOM diff in `src/compare.js` is a plain
// text diff between two runs of "the same dump function" rather than
// two structurally different serializations that would need their own
// normalization step before they're comparable at all. Kept in
// lockstep with the Rust version deliberately: same traversal order
// (document order, starting at `<html>`), same indentation (`| `,
// 2 spaces per depth), same attribute sorting, same comment/doctype
// exclusion (real DOM's `nodeType` 8/10, matching `blueice_dom` never
// materializing them as nodes), same text-node quoting.
//
// Passed directly to `page.evaluate(dumpDomInPage)`, which serializes
// the function to run inside the page's own JS context -- it has no
// access to this file's module scope there, so it must be entirely
// self-contained (no imports, no references to anything outside its
// own body).

export function dumpDomInPage() {
  function dumpNode(node, depth, out) {
    const indent = "  ".repeat(depth);
    if (node.nodeType === Node.ELEMENT_NODE) {
      const tag = node.tagName.toLowerCase();
      out.push(`| ${indent}<${tag}>`);
      const attrs = Array.from(node.attributes)
        .map((a) => [a.name, a.value])
        .sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
      const attrIndent = "  ".repeat(depth + 1);
      for (const [name, value] of attrs) {
        out.push(`| ${attrIndent}${name}="${value}"`);
      }
      for (const child of node.childNodes) {
        dumpNode(child, depth + 1, out);
      }
    } else if (node.nodeType === Node.TEXT_NODE) {
      out.push(`| ${indent}"${node.data}"`);
    }
    // Comment (8) and DocumentType (10) nodes are skipped, matching
    // blueice_dom not materializing them.
  }

  const out = [];
  dumpNode(document.documentElement, 0, out);
  return out.join("\n") + (out.length > 0 ? "\n" : "");
}
