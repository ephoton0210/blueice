// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Stage 2 (Chromium side) of `phase-15-chromium-differential-testing/
// PLAN.md` -- mirrors `capture-blueice.js` exactly (same fixture
// corpus, same per-fixture local HTTP server, same output-directory
// shape) but drives a real headless Chromium via Puppeteer instead of
// `blueice-mcp-server`, so `src/compare.js` can diff the two output
// trees fixture-by-fixture. Run with `npm run capture:chromium`.

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer";
import { dumpDomInPage } from "./dump-dom-in-page.js";
import { FIXTURES_DIR, loadFixtures } from "./fixtures.js";
import { serveHtml } from "./serve-fixture.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUTPUT_DIR = join(__dirname, "../output/chromium");

// Matches the viewport `blueice-mcp-server` spawns `blueice-core`
// with (`CoreProcess::spawn(800, 600)` in `main.rs`) -- both sides
// need the same viewport for the screenshots to be comparable at all,
// let alone diffable pixel-for-pixel.
const VIEWPORT = { width: 800, height: 600 };

function sanitizeForFilesystem(name) {
  return name.replace(/[^a-zA-Z0-9._-]/g, "_");
}

async function captureFixture(browser, fixture) {
  const server = await serveHtml(fixture.sections.data);
  let page;
  try {
    page = await browser.newPage();
    await page.setViewport(VIEWPORT);
    await page.goto(server.url, { waitUntil: "load" });

    const [dom, png] = await Promise.all([page.evaluate(dumpDomInPage), page.screenshot({ type: "png" })]);

    const dir = join(OUTPUT_DIR, sanitizeForFilesystem(fixture.name));
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "dom.txt"), dom);
    writeFileSync(join(dir, "screenshot.png"), png);
    console.log(`[${fixture.name}] captured dom.txt (${dom.length} bytes) and screenshot.png (${png.length} bytes)`);
    return true;
  } catch (err) {
    console.error(`[${fixture.name}] capture error: ${err.message}`);
    return false;
  } finally {
    if (page) await page.close();
    await server.close();
  }
}

async function main() {
  const fixtures = loadFixtures(FIXTURES_DIR).filter((f) => f.sections.data !== undefined);
  console.log(`loaded ${fixtures.length} fixture(s) with a #data section from ${FIXTURES_DIR}`);
  if (fixtures.length === 0) {
    throw new Error(`no fixtures found in ${FIXTURES_DIR}`);
  }

  const browser = await puppeteer.launch({ headless: true });
  console.log(`Chromium version: ${await browser.version()}`);
  mkdirSync(OUTPUT_DIR, { recursive: true });

  let failures = 0;
  try {
    for (const fixture of fixtures) {
      const ok = await captureFixture(browser, fixture);
      if (!ok) failures += 1;
    }
  } finally {
    await browser.close();
  }

  console.log(`done: ${fixtures.length - failures}/${fixtures.length} fixture(s) captured successfully`);
  if (failures > 0) process.exitCode = 1;
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
