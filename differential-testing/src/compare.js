// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Diffs `output/blueice/<fixture>/` against `output/chromium/<fixture>/`
// (both produced by `capture-blueice.js`/`capture-chromium.js`) and
// reports a similarity trend -- per
// `phase-15-chromium-differential-testing/PLAN.md`, this deliberately
// does *not* gate on exact equality: BlueIce's MVP scope is narrower
// than Chromium's by design (`phase-2-mvp-scope/PLAN.md`), so an
// exact-match assertion would just report permanent, expected
// failures forever. This script's exit code stays 0 regardless of how
// similar the two sides are -- it's a report, not a test.

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { diffLines } from "diff";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUTPUT_DIR = join(__dirname, "../output");
const BLUEICE_DIR = join(OUTPUT_DIR, "blueice");
const CHROMIUM_DIR = join(OUTPUT_DIR, "chromium");
const DIFF_IMAGE_DIR = join(OUTPUT_DIR, "diff");

/** Fraction of `a`'s lines that survive unchanged in `b` (1.0 = identical), via `diff`'s line-level diff. */
function domLineSimilarity(a, b) {
  const changes = diffLines(a, b);
  const totalLines = changes.reduce((sum, part) => sum + part.count, 0);
  if (totalLines === 0) return 1;
  const unchangedLines = changes.filter((part) => !part.added && !part.removed).reduce((sum, part) => sum + part.count, 0);
  return unchangedLines / totalLines;
}

function compareScreenshots(blueicePath, chromiumPath, diffOutPath) {
  const a = PNG.sync.read(readFileSync(blueicePath));
  const b = PNG.sync.read(readFileSync(chromiumPath));
  if (a.width !== b.width || a.height !== b.height) {
    return { comparable: false, note: `size mismatch: blueice ${a.width}x${a.height} vs chromium ${b.width}x${b.height}` };
  }
  const diff = new PNG({ width: a.width, height: a.height });
  const mismatched = pixelmatch(a.data, b.data, diff.data, a.width, a.height, { threshold: 0.1 });
  mkdirSync(dirname(diffOutPath), { recursive: true });
  writeFileSync(diffOutPath, PNG.sync.write(diff));
  return { comparable: true, mismatchedPixels: mismatched, totalPixels: a.width * a.height, matchFraction: 1 - mismatched / (a.width * a.height) };
}

function fixtureNames() {
  if (!existsSync(BLUEICE_DIR) || !existsSync(CHROMIUM_DIR)) {
    throw new Error(`both ${BLUEICE_DIR} and ${CHROMIUM_DIR} must exist -- run \`npm run capture:blueice\` and \`npm run capture:chromium\` first`);
  }
  const blueice = new Set(readdirSync(BLUEICE_DIR));
  const chromium = new Set(readdirSync(CHROMIUM_DIR));
  const both = [...blueice].filter((name) => chromium.has(name)).sort();
  const missing = { onlyBlueice: [...blueice].filter((n) => !chromium.has(n)), onlyChromium: [...chromium].filter((n) => !blueice.has(n)) };
  return { both, missing };
}

function main() {
  const { both, missing } = fixtureNames();
  if (missing.onlyBlueice.length > 0) console.warn(`captured by BlueIce only (missing from Chromium output): ${missing.onlyBlueice.join(", ")}`);
  if (missing.onlyChromium.length > 0) console.warn(`captured by Chromium only (missing from BlueIce output): ${missing.onlyChromium.join(", ")}`);

  const results = [];
  for (const name of both) {
    const blueiceDom = readFileSync(join(BLUEICE_DIR, name, "dom.txt"), "utf8");
    const chromiumDom = readFileSync(join(CHROMIUM_DIR, name, "dom.txt"), "utf8");
    const domIdentical = blueiceDom === chromiumDom;
    const domSimilarity = domIdentical ? 1 : domLineSimilarity(blueiceDom, chromiumDom);

    const screenshot = compareScreenshots(join(BLUEICE_DIR, name, "screenshot.png"), join(CHROMIUM_DIR, name, "screenshot.png"), join(DIFF_IMAGE_DIR, name, "screenshot-diff.png"));

    results.push({ fixture: name, domIdentical, domSimilarity, screenshot });
  }

  console.log("");
  console.log("fixture".padEnd(28), "dom".padEnd(10), "dom sim.".padEnd(10), "pixel match");
  for (const r of results) {
    const domCol = r.domIdentical ? "identical" : "differs";
    const domSimCol = `${(r.domSimilarity * 100).toFixed(1)}%`;
    const pixelCol = r.screenshot.comparable ? `${(r.screenshot.matchFraction * 100).toFixed(1)}%` : r.screenshot.note;
    console.log(r.fixture.padEnd(28), domCol.padEnd(10), domSimCol.padEnd(10), pixelCol);
  }

  const avg = (values) => values.reduce((a, b) => a + b, 0) / (values.length || 1);
  const summary = {
    fixturesCompared: results.length,
    domIdenticalCount: results.filter((r) => r.domIdentical).length,
    averageDomSimilarity: avg(results.map((r) => r.domSimilarity)),
    averagePixelMatch: avg(results.filter((r) => r.screenshot.comparable).map((r) => r.screenshot.matchFraction)),
  };
  console.log("");
  console.log(`summary: ${summary.domIdenticalCount}/${summary.fixturesCompared} DOM-identical, average DOM similarity ${(summary.averageDomSimilarity * 100).toFixed(1)}%, average pixel match ${(summary.averagePixelMatch * 100).toFixed(1)}%`);

  const reportPath = join(OUTPUT_DIR, "report.json");
  writeFileSync(reportPath, JSON.stringify({ summary, missing, results }, null, 2));
  console.log(`full report written to ${reportPath}`);
  console.log(`per-fixture screenshot diff images written under ${DIFF_IMAGE_DIR}`);
}

main();
