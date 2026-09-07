// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Stage 1 of `phase-15-chromium-differential-testing/PLAN.md`: drive
// BlueIce (via `blueice-mcp-server`) over the shared fixture corpus
// and capture its DOM dump + screenshot for each one. Deliberately
// has no Chromium/Puppeteer involvement yet -- that's stage 2, added
// once this half is proven to work on its own. Run with `npm run
// capture:blueice` (needs `cargo build --workspace` done first, since
// this spawns the compiled `blueice-mcp-server`/`blueice-core`
// binaries).

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { BlueIceClient } from "./blueice-client.js";
import { FIXTURES_DIR, loadFixtures } from "./fixtures.js";
import { serveHtml } from "./serve-fixture.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUTPUT_DIR = join(__dirname, "../output/blueice");

function sanitizeForFilesystem(name) {
  return name.replace(/[^a-zA-Z0-9._-]/g, "_");
}

async function captureFixture(client, fixture) {
  const server = await serveHtml(fixture.sections.data);
  try {
    const { error } = await client.navigate(server.url);
    if (error) {
      console.error(`[${fixture.name}] navigate error: ${error}`);
      return false;
    }

    const [dom, png] = await Promise.all([client.getDom(), client.screenshotPng()]);

    const dir = join(OUTPUT_DIR, sanitizeForFilesystem(fixture.name));
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "dom.txt"), dom);
    writeFileSync(join(dir, "screenshot.png"), png);
    console.log(`[${fixture.name}] captured dom.txt (${dom.length} bytes) and screenshot.png (${png.length} bytes)`);
    return true;
  } finally {
    await server.close();
  }
}

async function main() {
  const fixtures = loadFixtures(FIXTURES_DIR).filter((f) => f.sections.data !== undefined);
  console.log(`loaded ${fixtures.length} fixture(s) with a #data section from ${FIXTURES_DIR}`);
  if (fixtures.length === 0) {
    throw new Error(`no fixtures found in ${FIXTURES_DIR}`);
  }

  const client = new BlueIceClient();
  await client.connect();
  mkdirSync(OUTPUT_DIR, { recursive: true });

  let failures = 0;
  try {
    for (const fixture of fixtures) {
      const ok = await captureFixture(client, fixture);
      if (!ok) failures += 1;
    }
  } finally {
    await client.close();
  }

  console.log(`done: ${fixtures.length - failures}/${fixtures.length} fixture(s) captured successfully`);
  if (failures > 0) process.exitCode = 1;
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
