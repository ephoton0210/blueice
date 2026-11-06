// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
function verify(ts, view, item, options) {
  const helpers = item.env.sourceNodes.filter(node => item.blue
    ? ts.isFunctionDeclaration(node) && /^__blueice_target_(?:async|async_driver|generator|delegate_iterator|delegate_step)_\d+$/.test(node.name?.text)
    : ts.isVariableStatement(node) && node.declarationList.declarations.length === 1 && ['__awaiter', '__generator'].includes(node.declarationList.declarations[0].name.getText(view.source)));
  assert.equal(helpers.length, item.retained ? 0 : options.target === 'ES5' ? item.blue ? 4 : 2 : 1);
  if (item.blue && helpers.length) {
    const generators = helpers.filter(node => /^__blueice_target_(?:generator|delegate_iterator|delegate_step)_\d+$/.test(node.name.text));
    if (options.target === 'ES5') require('./ge5_binding_helpers.cjs').validate(ts, view, generators, item.identifiers); else assert.equal(generators.length, 0);
    const async = helpers.filter(node => !generators.includes(node)); assert.equal(async.length, 1);
    assert.equal(item.driver.expression.text, async[0].name.text);
    item.env.exact(async[0], fs.readFileSync(path.resolve(__dirname, '../../../../backend/bluets/src/emitter/targets/async_helpers.v1.js'), 'utf8').replaceAll('__blueice_target_async', async[0].name.text));
  }
  for (const node of helpers) view.noSource(node.getStart(view.source), node.end, 'complete class-method promise/generator helper');
  return helpers;
}
module.exports = {verify};
