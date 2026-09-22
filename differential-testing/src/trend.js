// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Compares compare.js's fresh report to the committed Phase 15 baseline and
// writes Markdown suitable for both local review and GITHUB_STEP_SUMMARY.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { renderTrend } from "./trend-lib.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const outputDir = join(__dirname, "../output");
const reportPath = process.env.DIFFERENTIAL_REPORT ?? join(outputDir, "report.json");
const baselinePath = process.env.DIFFERENTIAL_BASELINE ?? join(__dirname, "../baseline.json");
const trendPath = process.env.DIFFERENTIAL_TREND ?? join(outputDir, "trend.md");

function readJson(path, description) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`cannot read ${description} at ${path}: ${error.message}`);
  }
}

const current = readJson(reportPath, "current differential report");
const baseline = readJson(baselinePath, "committed differential baseline");
const baselineLabel = relative(process.cwd(), baselinePath) || baselinePath;
const markdown = renderTrend(current, baseline, { baselineLabel });
mkdirSync(dirname(trendPath), { recursive: true });
writeFileSync(trendPath, markdown);
process.stdout.write(markdown);
console.log(`trend written to ${trendPath}`);
