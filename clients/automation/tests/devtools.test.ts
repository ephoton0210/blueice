// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { test } from "node:test";
import assert from "node:assert/strict";
import { buildAccessibilityTree, formatAccessibilityTree, formatElementsTree } from "../src/devtools.ts";
import type { AccessibilityTree } from "../src/index.ts";

function snapshot(nodes: AccessibilityTree["nodes"]): AccessibilityTree {
  return { generation: 1, tab_id: 1, url: null, scroll_y: 0, nodes };
}

test("buildAccessibilityTree nests children under their real parent, roots first", () => {
  const tree = buildAccessibilityTree(
    snapshot([
      { id: 1, parent: null, children: [2], role: "generic", name: null },
      { id: 2, parent: 1, children: [], role: "button", name: "Save" },
    ]),
  );
  assert.equal(tree.length, 1);
  assert.equal(tree[0]!.id, 1);
  assert.equal(tree[0]!.children.length, 1);
  assert.equal(tree[0]!.children[0]!.id, 2);
  assert.equal(tree[0]!.children[0]!.label, 'button "Save"');
});

test("buildAccessibilityTree labels a nameless node with just its role", () => {
  const tree = buildAccessibilityTree(
    snapshot([{ id: 1, parent: null, children: [], role: "generic", name: null }]),
  );
  assert.equal(tree[0]!.label, "generic");
});

test("buildAccessibilityTree treats every parentless node as a root, in list order", () => {
  const tree = buildAccessibilityTree(
    snapshot([
      { id: 1, parent: null, children: [], role: "heading", name: "One" },
      { id: 2, parent: null, children: [], role: "heading", name: "Two" },
    ]),
  );
  assert.deepEqual(
    tree.map((n) => n.id),
    [1, 2],
  );
});

test("buildAccessibilityTree drops a child ID that doesn't resolve, instead of throwing", () => {
  const tree = buildAccessibilityTree(
    snapshot([{ id: 1, parent: null, children: [999], role: "generic", name: null }]),
  );
  assert.equal(tree[0]!.children.length, 0);
});

test("formatAccessibilityTree indents each depth by two spaces", () => {
  const text = formatAccessibilityTree(
    buildAccessibilityTree(
      snapshot([
        { id: 1, parent: null, children: [2], role: "main", name: null },
        { id: 2, parent: 1, children: [], role: "button", name: "Save" },
      ]),
    ),
  );
  assert.equal(text, 'main\n  button "Save"\n');
});

test("formatElementsTree returns the DOM dump verbatim", () => {
  const dump = '| <button>\n|   "Save"\n';
  assert.equal(formatElementsTree(dump), dump);
});
