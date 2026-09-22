// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Pure report validation and Markdown rendering for Phase 15's cross-run
// trend. Keeping it separate from the filesystem entry points gives the
// summary format focused Node tests without requiring Chromium or BlueIce.

function requireFiniteNumber(value, path) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new TypeError(`${path} must be a finite number`);
  }
}

/** Validates the stable subset of compare.js's report.json schema. */
export function validateReport(report, label = "report") {
  if (!report || typeof report !== "object") throw new TypeError(`${label} must be an object`);
  if (!report.summary || typeof report.summary !== "object") throw new TypeError(`${label}.summary must be an object`);
  if (!Array.isArray(report.results)) throw new TypeError(`${label}.results must be an array`);

  const { summary } = report;
  requireFiniteNumber(summary.fixturesCompared, `${label}.summary.fixturesCompared`);
  requireFiniteNumber(summary.domIdenticalCount, `${label}.summary.domIdenticalCount`);
  requireFiniteNumber(summary.averageDomSimilarity, `${label}.summary.averageDomSimilarity`);
  requireFiniteNumber(summary.averagePixelMatch, `${label}.summary.averagePixelMatch`);

  for (const [index, result] of report.results.entries()) {
    const path = `${label}.results[${index}]`;
    if (!result || typeof result !== "object" || typeof result.fixture !== "string") {
      throw new TypeError(`${path}.fixture must be a string`);
    }
    if (typeof result.domIdentical !== "boolean") throw new TypeError(`${path}.domIdentical must be a boolean`);
    requireFiniteNumber(result.domSimilarity, `${path}.domSimilarity`);
    if (result.screenshot?.comparable) requireFiniteNumber(result.screenshot.matchFraction, `${path}.screenshot.matchFraction`);
  }
  return report;
}

function percent(value) {
  return `${(value * 100).toFixed(2)}%`;
}

function signed(value, suffix = "") {
  const sign = value > 0 ? "+" : "";
  return `${sign}${value.toFixed(2)}${suffix}`;
}

function pixelMatch(result) {
  return result.screenshot?.comparable ? result.screenshot.matchFraction : null;
}

// Glyph rasterization can vary by a handful of pixels across otherwise
// identical runs. Keep that harmless noise out of the per-fixture change
// table; aggregate values still retain their full precision in report.json.
const PIXEL_CHANGE_EPSILON = 0.0001;

function escapeCell(value) {
  return String(value).replaceAll("|", "\\|");
}

/**
 * Renders a human-readable comparison. It intentionally has no threshold or
 * pass/fail result: divergence is evidence to inspect, not an expected-MVP
 * failure. Structural/malformed report errors are handled by validateReport.
 */
export function renderTrend(current, baseline, { baselineLabel = "baseline.json" } = {}) {
  validateReport(current, "current report");
  validateReport(baseline, "baseline report");

  const currentByFixture = new Map(current.results.map((result) => [result.fixture, result]));
  const baselineByFixture = new Map(baseline.results.map((result) => [result.fixture, result]));
  const added = [...currentByFixture.keys()].filter((name) => !baselineByFixture.has(name)).sort();
  const removed = [...baselineByFixture.keys()].filter((name) => !currentByFixture.has(name)).sort();

  const lines = [
    "# Chromium differential trend",
    "",
    `Baseline: \`${baselineLabel}\`. Similarity changes are informational and do not gate the job.`,
    "",
    "| Metric | Baseline | Current | Change |",
    "| --- | ---: | ---: | ---: |",
    `| Fixtures compared | ${baseline.summary.fixturesCompared} | ${current.summary.fixturesCompared} | ${signed(current.summary.fixturesCompared - baseline.summary.fixturesCompared)} |`,
    `| DOM-identical fixtures | ${baseline.summary.domIdenticalCount} | ${current.summary.domIdenticalCount} | ${signed(current.summary.domIdenticalCount - baseline.summary.domIdenticalCount)} |`,
    `| Average DOM similarity | ${percent(baseline.summary.averageDomSimilarity)} | ${percent(current.summary.averageDomSimilarity)} | ${signed((current.summary.averageDomSimilarity - baseline.summary.averageDomSimilarity) * 100, " pp")} |`,
    `| Average pixel match | ${percent(baseline.summary.averagePixelMatch)} | ${percent(current.summary.averagePixelMatch)} | ${signed((current.summary.averagePixelMatch - baseline.summary.averagePixelMatch) * 100, " pp")} |`,
    "",
  ];

  if (added.length > 0 || removed.length > 0) {
    lines.push("## Corpus changes", "");
    if (added.length > 0) lines.push(`- Added: ${added.map((name) => `\`${name}\``).join(", ")}`);
    if (removed.length > 0) lines.push(`- Removed: ${removed.map((name) => `\`${name}\``).join(", ")}`);
    lines.push("");
  }

  const changed = [];
  for (const [fixture, now] of currentByFixture) {
    const then = baselineByFixture.get(fixture);
    if (!then) continue;
    const domChanged = now.domIdentical !== then.domIdentical || now.domSimilarity !== then.domSimilarity;
    const nowPixel = pixelMatch(now);
    const thenPixel = pixelMatch(then);
    const pixelChanged = nowPixel === null || thenPixel === null
      ? nowPixel !== thenPixel
      : Math.abs(nowPixel - thenPixel) >= PIXEL_CHANGE_EPSILON;
    if (domChanged || pixelChanged) changed.push({ fixture, now, then, nowPixel, thenPixel });
  }

  lines.push("## Per-fixture changes", "");
  if (changed.length === 0) {
    lines.push("No existing fixture's DOM or comparable screenshot metric changed.");
  } else {
    lines.push("| Fixture | DOM | Pixel match |", "| --- | --- | --- | ");
    for (const { fixture, now, then, nowPixel, thenPixel } of changed.sort((a, b) => a.fixture.localeCompare(b.fixture))) {
      const dom = `${then.domIdentical ? "identical" : percent(then.domSimilarity)} → ${now.domIdentical ? "identical" : percent(now.domSimilarity)}`;
      const pixel = nowPixel === null || thenPixel === null
        ? `${thenPixel === null ? "not comparable" : percent(thenPixel)} → ${nowPixel === null ? "not comparable" : percent(nowPixel)}`
        : `${percent(thenPixel)} → ${percent(nowPixel)} (${signed((nowPixel - thenPixel) * 100, " pp")})`;
      lines.push(`| ${escapeCell(fixture)} | ${dom} | ${pixel} |`);
    }
  }

  return `${lines.join("\n")}\n`;
}
