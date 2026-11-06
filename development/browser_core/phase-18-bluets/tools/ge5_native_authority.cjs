// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Only the exact, separately reproduced 572-map corrections may replace stock.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const { sha256 } = require('./ge5_inputs.cjs');
let ledger;
function raw(reference, row, name) {
  const map = row.maps[name], original = reference.blobs[map.raw]; assert.equal(sha256(original), map.raw);
  if (!map.coordinateIssues.length) return original;
  if (!ledger) {
    const bytes = fs.readFileSync(path.resolve(__dirname, '../../../../backend/bluets/tests/fixtures/ge5_source_maps/native-map-defects.json'));
    assert.equal(sha256(bytes), '097a9b0df2857de39e2210e427c21f3dbcadebb4f8ed8d9dabbe5f0a87b04007');
    ledger = JSON.parse(bytes);
  }
  const defect = ledger.defects.find(item => item.caseId === row.id && item.map === name); assert(defect);
  assert.equal(defect.stockRawSha256, map.raw); assert.deepEqual(map.coordinateIssues, [defect.issue]);
  const corrected = ledger.patchedBlobs[defect.patchedRawSha256]; assert.equal(sha256(corrected), defect.patchedRawSha256);
  return corrected;
}
module.exports = { raw };
