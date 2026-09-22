#!/usr/bin/env node
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The Slice 1 item 4 "initial Elements/Accessibility DevTools panels,"
// as a real, runnable terminal inspection tool -- see `../src/
// devtools.ts`'s own module docs for why this is a terminal tool
// rather than a browser page in this slice. Prints the Elements panel
// (the tab's raw canonical DOM dump) and the Accessibility panel (a
// real, structured tree built from the tab's live accessibility
// snapshot) for one tab, then exits.
//
// Usage:
//   node devtools/inspect.ts <adapter-socket> <token-file> [tab-id]
//
// `tab-id` defaults to `1`, the default tab every fresh `core` process
// starts with.

import { readFileSync } from "node:fs";
import { Browser } from "../src/index.ts";
import { buildAccessibilityTree, formatAccessibilityTree, formatElementsTree } from "../src/devtools.ts";

async function main(): Promise<void> {
  const [socketPath, tokenFile, tabIdArg] = process.argv.slice(2);
  if (!socketPath || !tokenFile) {
    console.error("usage: node devtools/inspect.ts <adapter-socket> <token-file> [tab-id]");
    process.exitCode = 1;
    return;
  }
  const tabId = tabIdArg ? Number.parseInt(tabIdArg, 10) : 1;
  const token = readFileSync(tokenFile, "utf8");

  const browser = await Browser.connect({
    socketPath,
    token,
    clientName: "blueice-devtools-inspect",
  });
  try {
    const page = browser.page(tabId);

    console.log(`== Elements (tab ${tabId}) ==`);
    console.log(formatElementsTree(await page.dom()));

    console.log(`== Accessibility (tab ${tabId}) ==`);
    const tree = buildAccessibilityTree(await page.accessibilityTree());
    console.log(formatAccessibilityTree(tree));
  } finally {
    browser.close();
  }
}

main().catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
