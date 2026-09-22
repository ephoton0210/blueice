// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Runs a WPT CSS reftest pilot corpus against BlueIce's own renderer
// (phase-27-css-wpt-conformance/PLAN.md): for each reftest, navigate
// to the test page and its declared reference, screenshot both, and
// pixelmatch them. Needs no second engine, unlike
// `../capture-chromium.js` -- a `rel="match"` reference must render
// pixel-identically *to BlueIce's own renderer*, and a `rel="mismatch"`
// reference must not, regardless of what any other engine would show.

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";
import { BlueIceClient } from "../blueice-client.js";
import { serveHtml } from "../serve-fixture.js";
import { discoverReftests } from "./discover.js";
import { serveRoot } from "./serve-root.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "../../..");
const WPT_ROOT = join(REPO_ROOT, "development/browser_core/reference/wpt");
const PILOT_SUITE = process.env.CSS_WPT_SUITE ?? "css/css-color";
const OUTPUT_DIR = join(__dirname, "../../output/css-wpt");

function decodePng(buffer) {
  return PNG.sync.read(buffer);
}

/** Compares two same-size PNGs, returning the count of differing pixels. */
function diffPixelCount(a, b) {
  if (a.width !== b.width || a.height !== b.height) {
    // A real size mismatch is itself a failure to report, not silently
    // treated as either 0 or "every pixel" differing.
    return { sizeMismatch: true, count: Math.max(a.width * a.height, b.width * b.height) };
  }
  const diff = new PNG({ width: a.width, height: a.height });
  const count = pixelmatch(a.data, b.data, diff.data, a.width, a.height, { threshold: 0 });
  return { sizeMismatch: false, count, diff };
}

async function main() {
  if (!existsSync(WPT_ROOT)) {
    console.error(`WPT checkout not found at ${WPT_ROOT} -- see development/browser_core/reference/README.md`);
    process.exitCode = 1;
    return;
  }
  const suiteDir = join(WPT_ROOT, PILOT_SUITE);
  let reftests = discoverReftests(WPT_ROOT, suiteDir);
  console.log(`Discovered ${reftests.length} reftests under ${PILOT_SUITE}`);
  const limit = process.env.CSS_WPT_LIMIT ? Number(process.env.CSS_WPT_LIMIT) : undefined;
  if (limit) reftests = reftests.slice(0, limit);

  mkdirSync(OUTPUT_DIR, { recursive: true });
  const server = await serveRoot(WPT_ROOT);
  const client = new BlueIceClient();
  await client.connect();

  // A reftest whose test and reference both use CSS BlueIce's MVP scope
  // doesn't parse (e.g. `rgb()`/`hsl()` functional color notation, which
  // `backend/core/css` doesn't implement at all) can "pass" by both
  // sides silently rendering the *same unstyled default* rather than by
  // genuinely agreeing on a real computed value -- a false positive a
  // raw pass count can't distinguish from a true one. Screenshotting one
  // truly blank page up front gives every result something real to
  // compare against: a passing reftest whose own screenshot matches this
  // blank baseline is flagged `blankEquivalent`, since its "pass" proves
  // nothing about the feature it claims to test.
  const blankPage = await serveHtml("<!doctype html><html><body></body></html>");
  await client.navigate(blankPage.url);
  const blankPng = decodePng(await client.screenshotPngWhenReady());
  await blankPage.close();

  const results = [];
  try {
    for (const [index, reftest] of reftests.entries()) {
      const testUrl = `${server.url}/${reftest.testPath}`;
      const refUrl = `${server.url}/${reftest.refPath}`;
      process.stdout.write(`\r[${index + 1}/${reftests.length}] ${reftest.testPath}`.padEnd(100));
      try {
        const testNav = await client.navigate(testUrl);
        if (testNav.error) throw new Error(`navigate(test) failed: ${testNav.error}`);
        const testPng = decodePng(await client.screenshotPngWhenReady());
        const refNav = await client.navigate(refUrl);
        if (refNav.error) throw new Error(`navigate(ref) failed: ${refNav.error}`);
        const refPng = decodePng(await client.screenshotPngWhenReady());
        const { sizeMismatch, count, diff } = diffPixelCount(testPng, refPng);
        const identical = !sizeMismatch && count === 0;
        const pass = reftest.kind === "match" ? identical : !identical;
        const blankEquivalent = pass && diffPixelCount(testPng, blankPng).count === 0;
        results.push({ ...reftest, pass, sizeMismatch, diffPixels: count, blankEquivalent });
        if (!pass && diff) {
          const diffPath = join(OUTPUT_DIR, reftest.testPath.replace(/[/\\]/g, "_") + ".diff.png");
          writeFileSync(diffPath, PNG.sync.write(diff));
        }
      } catch (error) {
        results.push({ ...reftest, pass: false, error: String(error?.message ?? error) });
      }
    }
  } finally {
    process.stdout.write("\n");
    await client.close();
    await server.close();
  }

  const passed = results.filter((r) => r.pass).length;
  const failed = results.filter((r) => !r.pass && !r.error).length;
  const errored = results.filter((r) => r.error).length;
  const blankEquivalentPasses = results.filter((r) => r.blankEquivalent).length;
  const genuinePasses = passed - blankEquivalentPasses;
  console.log(
    `\n${PILOT_SUITE}: ${passed} / ${results.length} raw pass (${((100 * passed) / results.length).toFixed(1)}%), ${failed} fail, ${errored} errored\n` +
      `  of those passes, ${blankEquivalentPasses} render identically to a blank page on both sides (uninformative -- see "blankEquivalent" in report.json)\n` +
      `  genuine passes (render something, and it matches): ${genuinePasses} / ${results.length} (${((100 * genuinePasses) / results.length).toFixed(1)}%)`
  );

  writeFileSync(join(OUTPUT_DIR, "report.json"), JSON.stringify({ suite: PILOT_SUITE, total: results.length, passed, failed, errored, blankEquivalentPasses, genuinePasses, results }, null, 2));
  console.log(`Full report: ${join(OUTPUT_DIR, "report.json")}`);
}

main();
