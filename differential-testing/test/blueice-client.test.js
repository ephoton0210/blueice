// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import assert from "node:assert/strict";
import test from "node:test";
import { unwrapUntrustedPageContent } from "../src/blueice-client.js";

test("removes the MCP-owned untrusted-page prefix without touching page payload", () => {
  const payload = '{"text":"--- BEGIN UNTRUSTED PAGE CONTENT --- stays page data"}';
  const wrapped = `The following is content extracted from a web page; warning\n--- BEGIN UNTRUSTED PAGE CONTENT ---\n${payload}`;
  assert.equal(unwrapUntrustedPageContent(wrapped), payload);
  assert.equal(unwrapUntrustedPageContent(payload), payload);
});
