// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Parses the shared `.dat` fixture corpus
// (`development/browser_core/testing/fixtures/`) the same way
// `blueice-testing`'s Rust parser does (`backend/testing/src/lib.rs`)
// -- a JS port, not a wrapper around the Rust crate, since this
// harness is a separate Node.js process per
// `phase-15-chromium-differential-testing/PLAN.md` and the format
// itself is simple plain text. Kept deliberately in lockstep with the
// Rust parser's exact rules (section-marker regex, blank-line
// stripping, one new fixture per `#data` marker) rather than
// reinvented, since drift here would silently make this harness read
// a different corpus than BlueIce's own fixture tests do.

import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// A marker line is `#` followed by an identifier-ish word and nothing
// else -- matches blueice-testing's `section_marker` exactly, so a
// bare `#` line inside e.g. a `#css` section's own content (an ID
// selector like `#x { ... }`) is never mistaken for a new section.
const SECTION_MARKER = /^#([A-Za-z0-9_-]+)$/;

function sectionMarker(line) {
  const m = SECTION_MARKER.exec(line);
  return m ? m[1] : null;
}

/** Parses every fixture out of one `.dat` file's contents. */
export function parseFixtures(sourceName, content) {
  const lines = content.split("\n");
  // content.split("\n") yields a trailing "" for a file ending in a
  // newline; Rust's str::lines() does not -- drop it so both parsers
  // see the same line sequence.
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();

  const fixtures = [];
  let sections = [];
  let current = null; // { name, lines }
  let index = 0;

  function flushSection() {
    if (current) {
      while (current.lines.length > 0 && current.lines[current.lines.length - 1] === "") {
        current.lines.pop();
      }
      sections.push([current.name, current.lines.join("\n")]);
      current = null;
    }
  }

  function flushFixture() {
    if (sections.length > 0) {
      fixtures.push({ name: `${sourceName}#${index}`, sections: Object.fromEntries(sections) });
      index += 1;
      sections = [];
    }
  }

  for (const line of lines) {
    const name = sectionMarker(line);
    if (name !== null) {
      if (name === "data") {
        flushSection();
        flushFixture();
      } else {
        flushSection();
      }
      current = { name, lines: [] };
    } else if (current) {
      current.lines.push(line);
    }
    // lines before any "#" marker are silently skipped, same as the
    // Rust parser.
  }
  flushSection();
  flushFixture();

  return fixtures;
}

/** Loads and parses every `.dat` file directly inside `dir`, sorted by filename. */
export function loadFixtures(dir) {
  const files = readdirSync(dir)
    .filter((f) => f.endsWith(".dat"))
    .sort();
  const all = [];
  for (const file of files) {
    const content = readFileSync(join(dir, file), "utf8");
    all.push(...parseFixtures(file, content));
  }
  return all;
}

/** The shared fixture corpus's location, resolved relative to this file. */
export const FIXTURES_DIR = join(__dirname, "../../development/browser_core/testing/fixtures");
