// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import assert from "node:assert/strict";
import test from "node:test";
import { renderTrend, validateReport } from "../src/trend-lib.js";

function report({ dom = 0.9, pixel = 0.8, fixture = "basic.dat_0" } = {}) {
  return {
    summary: {
      fixturesCompared: 1,
      domIdenticalCount: dom === 1 ? 1 : 0,
      averageDomSimilarity: dom,
      averagePixelMatch: pixel,
    },
    results: [{
      fixture,
      domIdentical: dom === 1,
      domSimilarity: dom,
      screenshot: { comparable: true, matchFraction: pixel },
    }],
  };
}

test("renders aggregate and per-fixture metric changes without a pass/fail threshold", () => {
  const markdown = renderTrend(report({ dom: 0.95, pixel: 0.85 }), report(), { baselineLabel: "baseline.json" });
  assert.match(markdown, /\| Average DOM similarity \| 90\.00% \| 95\.00% \| \+5\.00 pp \|/);
  assert.match(markdown, /\| Average pixel match \| 80\.00% \| 85\.00% \| \+5\.00 pp \|/);
  assert.match(markdown, /basic\.dat_0/);
  assert.match(markdown, /informational and do not gate the job/);
});

test("reports fixture-corpus additions separately from metric changes", () => {
  const current = report({ fixture: "new.dat_0" });
  const markdown = renderTrend(current, report(), { baselineLabel: "baseline.json" });
  assert.match(markdown, /## Corpus changes/);
  assert.match(markdown, /Added: `new\.dat_0`/);
  assert.match(markdown, /Removed: `basic\.dat_0`/);
});

test("does not elevate a few antialiased pixels into a per-fixture change", () => {
  const markdown = renderTrend(report({ pixel: 0.800001 }), report(), { baselineLabel: "baseline.json" });
  assert.match(markdown, /No existing fixture's DOM or comparable screenshot metric changed\./);
});

test("rejects malformed reports before rendering a misleading trend", () => {
  assert.throws(() => validateReport({ summary: {}, results: [] }), /fixturesCompared/);
});
