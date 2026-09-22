// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Runs the actual `devtools/inspect.ts` CLI as a real subprocess
// against a real spawned `blueice-core`/`blueice-automation` pair --
// the process-wiring half of the Slice 1 item 4 "initial Elements/
// Accessibility DevTools panels" deliverable, complementing
// `devtools.test.ts`'s unit tests of the pure tree-building/formatting
// functions the CLI itself just prints.

import { test } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnCoreAndAdapter } from "./harness.ts";

const execFileAsync = promisify(execFile);
const inspectScript = join(fileURLToPath(import.meta.url), "..", "..", "devtools", "inspect.ts");

test("the inspect CLI prints a real Elements and Accessibility panel for the default tab", async (t) => {
  const harness = await spawnCoreAndAdapter("cli");
  t.after(harness.cleanup);

  const { stdout } = await execFileAsync(process.execPath, [
    inspectScript,
    harness.automationSocket,
    harness.tokenFile,
    "1",
  ]);

  assert.match(stdout, /== Elements \(tab 1\) ==/);
  assert.match(stdout, /<button>/);
  assert.match(stdout, /== Accessibility \(tab 1\) ==/);
  assert.match(stdout, /"Save"/);
});

test("the inspect CLI reports a clear usage error when arguments are missing", async () => {
  await assert.rejects(execFileAsync(process.execPath, [inspectScript]), (error: unknown) => {
    const message = String((error as { stderr?: string }).stderr ?? error);
    assert.match(message, /usage: node devtools\/inspect\.ts/);
    return true;
  });
});
