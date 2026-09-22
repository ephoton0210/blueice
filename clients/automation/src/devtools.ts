// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The pure data/formatting layer behind Slice 1 item 4's "initial
// Elements/Accessibility DevTools panels" -- deliberately scoped down
// from an interactive graphical UI to a real, terminal-rendered
// inspection tool (`devtools/inspect.ts`, this module's own consumer)
// for one structural reason, not a shortcut: a browser page cannot
// open a raw Unix-domain socket at all (no such API exists in a
// browser sandbox), so an HTML/JS DevTools panel would need its own
// WebSocket-bridge relay process in front of `@blueice/automation`'s
// direct-socket transport -- a separate piece of infrastructure this
// slice doesn't build yet. What's real here: genuine data from a live
// page (the Accessibility tree's actual parent/children structure,
// not a placeholder), and every non-trivial transformation is a pure,
// independently unit-tested function. The interactive/graphical panel
// itself (and the transport it would need) is later work, the same
// "documented, deliberate deferral" this project applies elsewhere
// (see `blueice_ipc::automation`'s own module docs on `Evaluate`/
// `ApiWorkspaceSend`).

import type { AccessibilityTree } from "./index.ts";

/** One node of an `AccessibilityTree`, typed just enough for this
 * module's own purposes -- deliberately not a full mirror of
 * `blueice_ipc::AiNode` (see `AccessibilityTree`'s own docs for why). */
interface AccessibilityFlatNode {
  id: number;
  parent: number | null;
  children: number[];
  role: unknown;
  name?: string | null;
}

export interface AccessibilityTreeNode {
  id: number;
  label: string;
  children: AccessibilityTreeNode[];
}

/**
 * Converts a flat `AccessibilityTree` (a `blueice_ipc::AiSnapshot`'s
 * own wire shape -- a flat list addressed by ID, per that type's own
 * "simpler to look things up in than a recursive structure" docs)
 * into the nested tree a panel actually renders, via each node's own
 * `parent`/`children` ID fields. Nodes whose `parent` is absent from
 * the snapshot (should only ever be the root's immediate children,
 * whose `parent` is `null`) are treated as roots -- a node ID this
 * function can't resolve is silently dropped rather than throwing,
 * since a genuinely malformed/partial snapshot is a data problem this
 * pure formatting layer shouldn't crash a caller over.
 */
export function buildAccessibilityTree(snapshot: AccessibilityTree): AccessibilityTreeNode[] {
  const nodes = snapshot.nodes as AccessibilityFlatNode[];
  const byId = new Map(nodes.map((node) => [node.id, node]));

  function toTreeNode(node: AccessibilityFlatNode): AccessibilityTreeNode {
    const label = node.name ? `${String(node.role)} "${node.name}"` : String(node.role);
    const children = node.children
      .map((childId) => byId.get(childId))
      .filter((child): child is AccessibilityFlatNode => child !== undefined)
      .map(toTreeNode);
    return { id: node.id, label, children };
  }

  return nodes.filter((node) => node.parent === null).map(toTreeNode);
}

/** Renders a tree from {@link buildAccessibilityTree} as indented
 * plain text, e.g.:
 * ```
 * main
 *   button "Save"
 * ```
 */
export function formatAccessibilityTree(nodes: AccessibilityTreeNode[], depth = 0): string {
  const indent = "  ".repeat(depth);
  return nodes
    .map((node) => {
      const own = `${indent}${node.label}\n`;
      return own + formatAccessibilityTree(node.children, depth + 1);
    })
    .join("");
}

/**
 * The Elements panel's own "formatting": verbatim, unmodified --
 * `blueice_dom::dump`'s text format is this project's one canonical,
 * diffable serialization of a DOM tree (also used for Chromium
 * differential testing), so re-parsing it into a second tree structure
 * here would be a second implementation of that format with its own
 * chance to silently drift from the real one, for no benefit a plain
 * `<pre>`-equivalent rendering doesn't already get. Exists as its own
 * named function (rather than callers just using `page.dom()`
 * directly) so the panel's own formatting choice is documented in one
 * place.
 */
export function formatElementsTree(domDump: string): string {
  return domDump;
}
