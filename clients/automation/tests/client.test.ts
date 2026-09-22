// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exercises `@blueice/automation` against the real compiled
// `blueice-core` and `blueice-automation` binaries -- the "e2e test
// through the real public interface" the project's Definition of Done
// asks for, one language over from the Rust-side subprocess tests
// this mirrors (`backend/core/engine/tests/core_binary.rs`,
// `backend/automation/tests/automation_binary.rs`). See `./harness.ts`
// for the shared spawn/setup this and `devtools_inspect_cli.test.ts`
// both use.

import { test } from "node:test";
import assert from "node:assert/strict";
import { Browser, AutomationError } from "../src/index.ts";
import { spawnCoreAndAdapter } from "./harness.ts";

test("a real Browser can inspect, locate and click an element on the default tab", async (t) => {
  const harness = await spawnCoreAndAdapter("ok");
  t.after(harness.cleanup);

  const browser = await Browser.connect({
    socketPath: harness.automationSocket,
    token: harness.token,
  });
  t.after(() => browser.close());

  assert.ok(browser.grantedCapabilities.includes("Inspection"));

  const page = browser.page(1); // the default tab, navigated in setup

  const dom = await page.dom();
  assert.match(dom, /button/);

  const tree = await page.accessibilityTree();
  assert.equal(tree.tab_id, 1);
  const names = (tree.nodes as { name?: string }[]).map((n) => n.name);
  assert.ok(names.includes("Save"), `expected a "Save" node, got ${JSON.stringify(names)}`);

  const png = await page.screenshot();
  assert.deepEqual(png.subarray(0, 8), Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));

  const locator = page.locator("#save");
  const nodeIds = await locator.resolve();
  assert.equal(nodeIds.length, 1);

  await assert.rejects(() => locator.click(), (error: unknown) => {
    assert.ok(error instanceof AutomationError);
    assert.equal(error.detail, "NoControllerLease");
    return true;
  });

  const lease = await browser.acquireControllerLease();
  await locator.click(); // must not throw now that a lease is held
  await browser.releaseControllerLease(lease);
});

test("BrowserContext.newPage opens a real, independently-blank tab", async (t) => {
  const harness = await spawnCoreAndAdapter("ctx");
  t.after(harness.cleanup);

  const browser = await Browser.connect({
    socketPath: harness.automationSocket,
    token: harness.token,
  });
  t.after(() => browser.close());

  const context = await browser.newContext();
  const page = await context.newPage();
  assert.notEqual(page.tabId, 1, "a freshly opened tab must not reuse the default tab's ID");
  assert.equal(await page.dom(), "", "a freshly opened, never-navigated tab has an empty DOM");

  await context.close();
  await assert.rejects(() => page.dom());
});

test("a wrong token is rejected by the adapter before reaching core", async (t) => {
  const harness = await spawnCoreAndAdapter("bad");
  t.after(harness.cleanup);

  await assert.rejects(() =>
    Browser.connect({ socketPath: harness.automationSocket, token: "definitely-not-it" }),
  );
});
