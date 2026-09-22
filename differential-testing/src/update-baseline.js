// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Deliberately explicit maintenance command. A new baseline is an intentional
// review decision, never an automatic side effect of a differential run.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { validateReport } from "./trend-lib.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const outputDir = join(__dirname, "../output");
const reportPath = process.env.DIFFERENTIAL_REPORT ?? join(outputDir, "report.json");
const baselinePath = process.env.DIFFERENTIAL_BASELINE ?? join(__dirname, "../baseline.json");

let report;
try {
  report = JSON.parse(readFileSync(reportPath, "utf8"));
} catch (error) {
  throw new Error(`cannot read current differential report at ${reportPath}: ${error.message}`);
}
validateReport(report, "current report");
mkdirSync(dirname(baselinePath), { recursive: true });
writeFileSync(baselinePath, `${JSON.stringify({ schemaVersion: 1, ...report }, null, 2)}\n`);
console.log(`updated committed differential baseline at ${baselinePath}`);
