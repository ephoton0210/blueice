// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independently covered outer signatures precede all source-machine capabilities.
const assert = require('node:assert/strict'), {compareTokens} = require('./ge5_role_stream.cjs');
function compare(ts, native, np, actual, ap, origin, capabilities) {
  const header = (view, fn) => view.tokens(fn).filter(token => token.end <= fn.body.getStart(view.source) + 1);
  compareTokens(native, header(native, np.fn), actual, header(actual, ap.fn), 'complete original generator signature');
  const model = require('./ge5_generator_switch_machine.cjs');
  const capability = model.create(ts, native, capabilities.native, actual, capabilities.actual, origin);
  const n = model.read(ts, capability, 'native'), a = model.read(ts, capability, 'blue');
  const proof = require('./ge5_linear_roles.cjs').compareFunction(ts, native, n, actual, a);
  assert.equal(proof.nativePaths.length, 3); assert.equal(proof.actualPaths.length, 3);
  for (let path = 0; path < proof.nativePaths.length; path++) {
    const events = proof.nativePaths[path].events, peers = proof.actualPaths[path].events;
    for (let index = 0; index < events.length; index++) {
      const event = events[index], peer = peers[index];
      for (const key of event.kind === 'case' ? ['value', 'test'] : event.kind === 'jump' ? [] : ['value']) {
        assert.deepEqual(actual.owner(peer[key].node.end), native.owner(event[key].node.end), 'complete original ' + event.kind + '/' + key + ' operand endpoint');
      }
      if (event.kind === 'case' && !peer.projection) {
        const colon = native.tokens(event.clause).find(t => t.text === ':');
        const counterpart = actual.tokens(peer.clause).find(t => t.text === ':'); assert(colon && counterpart);
        assert.deepEqual(actual.owner(counterpart.end), native.owner(colon.end), 'complete source-case punctuation phase');
      }
    }
  }
  return {...proof, capability};
}
module.exports = {compare};
