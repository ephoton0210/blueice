// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Finds WPT reftests inside a directory: any `.html`/`.xht` file
// declaring `<link rel="match"|"mismatch" href="...">` in its own
// markup. A reference file itself (`*-ref.html` by convention, though
// this looks at the actual `rel` links rather than trusting the
// filename) is never picked up as a test of its own unless it too
// declares a `rel="match"`/`"mismatch"` link -- rare, but real WPT
// reference chains do this occasionally; this harness resolves only
// the one direct link a test declares, not multi-hop chains.

import { readFileSync, readdirSync } from "node:fs";
import { extname, join, posix, relative, sep } from "node:path";

const LINK_RE = /<link\s+[^>]*rel=["']?(match|mismatch)["']?[^>]*>/gi;
const HREF_RE = /href=["']([^"']+)["']/i;
const TESTHARNESS_RE = /testharness\.js/i;

/**
 * Returns every reftest under `dir` (recursing into subdirectories),
 * each as `{ testPath, refPath, kind }` -- paths are relative to `root`
 * (POSIX-style, for building request URLs), `kind` is `"match"` or
 * `"mismatch"`. Files whose only reftest links are alongside a
 * `testharness.js` include are skipped -- WPT occasionally uses both
 * in one file, and a script-driven assertion inside it is exactly the
 * CSSOM-dependent case `phase-27-css-wpt-conformance/PLAN.md` excludes.
 */
export function discoverReftests(root, dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...discoverReftests(root, full));
      continue;
    }
    if (![".html", ".htm", ".xht", ".xhtml"].includes(extname(entry.name).toLowerCase())) continue;
    const source = readFileSync(full, "utf8");
    if (TESTHARNESS_RE.test(source)) continue;
    const links = [...source.matchAll(LINK_RE)];
    if (links.length === 0) continue;
    // A test may declare more than one reference; this harness checks
    // the first one only, matching this phase's documented v1 scope
    // (a stated simplification, not a silent one).
    const [linkTag, kind] = links[0];
    const hrefMatch = HREF_RE.exec(linkTag);
    if (!hrefMatch) continue;
    const testPath = toPosix(relative(root, full));
    out.push({
      testPath,
      refPath: normalizeRefPath(testPath, hrefMatch[1]),
      kind,
    });
  }
  return out;
}

function toPosix(p) {
  return p.split(sep).join("/");
}

/** Resolves `href` (relative or root-absolute) against `testPath`. */
function normalizeRefPath(testPath, href) {
  if (href.startsWith("/")) return href.slice(1);
  const dir = posix.dirname(testPath);
  return posix.normalize(posix.join(dir, href));
}
