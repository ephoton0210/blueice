// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * Compares two gzip+base64 CLDR provider files by their decompressed payload.
 *
 * Usage:
 *   node backend/ecma402/tools/compare_cldr_payloads.mjs expected.b64 actual.b64
 *
 * Exits 0 when the decompressed bytes are identical, 1 when they differ. The
 * files themselves are deliberately not compared byte for byte: the gzip
 * stream depends on the zlib build bundled with a given Node release, so two
 * runs can encode an identical table to different bytes.
 */

import fs from 'node:fs';
import zlib from 'node:zlib';

const [expectedPath, actualPath] = process.argv.slice(2);
if (!expectedPath || !actualPath) {
  throw new Error('usage: compare_cldr_payloads.mjs <expected.b64> <actual.b64>');
}

const payload = (file) => zlib.gunzipSync(Buffer.from(fs.readFileSync(file, 'utf8'), 'base64'));
const expected = payload(expectedPath);
const actual = payload(actualPath);
if (!expected.equals(actual)) {
  console.error(
    `CLDR payload mismatch: ${expectedPath} (${expected.length} bytes) != ${actualPath} (${actual.length} bytes)`,
  );
  process.exit(1);
}
