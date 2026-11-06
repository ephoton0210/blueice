// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Check each complete helper body and its closed lexical dependency identities.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
function validate(ts, view, nodes, originals) {
  const directory = path.resolve(__dirname, '../../../../backend/bluets/src/emitter/targets');
  const source = ts.createSourceFile('generator-abi.js',
    fs.readFileSync(path.join(directory, 'generator_helpers.v1.js'), 'utf8')
      + fs.readFileSync(path.join(directory, 'generator_delegation_helpers.v1.js'), 'utf8'),
    ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  const expected = new Map(source.statements.map(node => [
    ({ __blueice_target_generator: 'generator', __blueice_target_iterator: 'delegate_iterator',
      __blueice_target_step_result: 'delegate_step' })[node.name.text], node,
  ]));
  assert.equal(expected.size, 3);
  const aliases = new Map(), roles = new Map();
  for (const node of nodes) {
    assert(ts.isFunctionDeclaration(node) && node.name);
    const name = node.name.text, role = /^__blueice_target_(generator|delegate_iterator|delegate_step)_\d+$/.exec(name)?.[1];
    assert(role && !aliases.has(name));
    assert(!originals.some(original => new RegExp('\\b' + name + '\\b').test(original)), 'fresh generated helper binding');
    aliases.set(name, '$' + role);
    if (!roles.has(role)) roles.set(role, []);
    roles.get(role).push(node);
  }
  assert(roles.get('generator')?.length > 0);
  assert.equal(roles.get('delegate_iterator')?.length, roles.get('generator').length);
  assert.equal(roles.get('delegate_step')?.length, roles.get('generator').length);
  const wanted = new Map([
    ['__blueice_target_generator', '$generator'], ['__blueice_target_iterator', '$delegate_iterator'],
    ['__blueice_target_step_result', '$delegate_step'],
  ]);
  function tokens(node, file, names) {
    const result = [];
    function visit(node) {
      if (ts.isToken(node)) {
        if (node.kind !== ts.SyntaxKind.EndOfFileToken) result.push(names.get(node.getText(file)) || node.getText(file));
        return;
      }
      for (const child of node.getChildren(file)) visit(child);
    }
    visit(node); return result;
  }
  for (const [role, group] of roles) for (const node of group) {
    assert.deepEqual(tokens(node, view.source, aliases), tokens(expected.get(role), source, wanted),
      'complete ' + role + ' helper ABI and dependency bindings');
    view.noSource(node.getStart(view.source), node.end, 'complete generated ' + role + ' helper');
  }
}
module.exports = { validate };
