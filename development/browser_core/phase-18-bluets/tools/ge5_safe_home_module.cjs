// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every private phase must close before the shared enclosing module can finish.
const assert = require('node:assert/strict'), ir = require('./ge5_safe_home_ir.cjs');
function compare(ts, native, actual, options, originalText) {
  let previous;
  for (const point of actual.decoded.segments) {
    assert(!previous || point[0] > previous[0] || point[0] === previous[0] && point[1] > previous[1],
      'strict ordered unique decoded safe-home map entries');
    previous = point;
  }
  let cap, n, a, helpers, prelude, frames, metadata;
  const proof = require('./ge5_class_module.cjs').compare(ts, native, actual, options, originalText, {
    prepare({original, contract}) {
      cap = ir.source(ts, original); n = ir.read(ts, native, cap, false); a = ir.read(ts, actual, cap, true);
      require('./ge5_safe_home_artifacts.cjs').compare(ts, n, a, options);
      helpers = require('./ge5_safe_home_helpers.cjs').compare(ts, n, a, options);
      prelude = require('./ge5_class_expression_prelude.cjs').compare(ts, native, actual, original, options, contract);
      assert.equal(prelude.deferred.length, 0, 'complete original source callable prelude inventory');
      frames = require('./ge5_safe_home_frames.cjs').compare(ts, n, a, options, contract);
      const definitions = n.entries.map((entry, index) => require('./ge5_safe_home_pairs.cjs').definition(ts, n, entry, a, a.entries[index], options, contract));
      metadata = require('./ge5_safe_home_metadata.cjs').compare(ts, n, a, contract, definitions, helpers);
      return {definitions, prelude, helpers, classProofs: prelude.classes.filter(p => p.classProof).map(p => ({...p.classProof, native: {...p.classProof.native, node: p.native.node}}))};
    },
    compareClass({source, contract}) {
      if (source !== cap.cls) return null;
      return require('./ge5_safe_home_class.cjs').compare(ts, n, a, options, contract, frames, metadata);
    },
  });
  const endings = require('./ge5_safe_home_endings.cjs').compare(ts, n, a, proof);
  return {...proof, endings, cap, private: {native: n, actual: a}, helpers, prelude, completeModule: true};
}
module.exports = {compare};
