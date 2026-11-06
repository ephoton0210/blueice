// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Close every original declaration, acquisition, publication and generated region.
const assert = require('node:assert/strict');
const sources = require('./ge5_interop_source.cjs'), envelope = require('./ge5_interop_envelope.cjs');
const imports = require('./ge5_interop_imports.cjs'), helpers = require('./ge5_interop_helpers.cjs');
const acquisitions = require('./ge5_interop_acquisitions.cjs'), declarations = require('./ge5_interop_declaration_maps.cjs');
const accounting = require('./ge5_class_accounting.cjs'), publications = require('./ge5_interop_publications.cjs');
const systemTransport = require('./ge5_interop_system_transport.cjs');

function compare(ts, native, actual, options, files, entry, filename, actualBlue = true) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
  if (actualBlue) {
    let previous = null;
    for (const point of actual.decoded.segments) {
      assert(!previous || point[0] > previous[0] || point[0] === previous[0] && point[1] > previous[1],
        'complete actual module decoded segments are ordered and unique');
      previous = point;
    }
  }
  if (native.code === actual.code) assert.deepEqual(actual.decoded, native.decoded, 'identical output complete decoded map');
  const original = sources.read(ts, files, entry), module = sources.checked(original).modules[filename]; assert(module);
  const comments = require('./ge5_interop_import_comments.cjs').project(ts, original, filename, options, native, actual);
  const roles = accounting.create(ts, native, actual, [comments.text]);
  const models = [native, actual].map((view, index) => {
    const blue = index === 1 && actualBlue, item = envelope.read(ts, view, options, original, filename, blue);
    item.classifyTransport();
    const binding = imports.read(ts, item, original, filename, blue), helper = helpers.read(ts, item, original, filename, options, blue);
    const model = imports.checked(binding, view), acquisition = acquisitions.read(ts, binding, helper, view, options, blue);
    const consumed = new Set([...acquisition.consumed, ...helpers.checked(helper, view).generated, ...item.generated]);
    return {view, blue, item, binding, helper, model, acquisition, consumed};
  });
  const [n, a] = models, proof = declarations.compare(ts, native, n.binding, actual, a.binding, roles);
  for (const [index, model] of models.entries()) {
    for (const declaration of (index ? proof.actual : proof.native).declarations) model.consumed.add(declaration.statement);
  }
  function uniform(view, node, owner, label) {
    const start = node.getStart(view.source);
    for (let offset = start; offset < node.end; offset++) assert.deepEqual(view.owner(offset), owner, label);
    view.cover(start, node.end); view.roleStarts.set(start, owner); view.roleEnds.set(node.end, view.owner(node.end));
  }
  function sourcePoint(offset) { const position = module.source.getLineAndCharacterOfPosition(offset); return [0, position.line, position.character]; }
  function inlineStar(model, acquisition) {
    const {item, view, consumed} = model, namespace = acquisition.namespace;
    const candidates = item.body.filter(node => ts.isForInStatement(node) && ts.isIdentifier(node.expression) && node.expression.text === namespace);
    assert.equal(candidates.length, 1, 'one complete original inline star publication');
    const loop = candidates[0], operation = item.format === 'system' ? item.system.stem + '_define' : 'Object.defineProperty';
    item.exact(loop, `for(${options.target === 'ES5' ? 'var' : 'const'} key in ${namespace}){if(key!=='default'&&!Object.prototype.hasOwnProperty.call(exports,key))(function(captured){${operation}(exports,captured,{enumerable:true,get:function(){return ${namespace}[captured];}});})(key);}`);
    consumed.add(loop); return loop;
  }
  const acquisitionPairs = [];
  for (let index = 0; index < n.acquisition.edges.length; index++) {
    const x = n.acquisition.edges[index], y = a.acquisition.edges[index]; assert.equal(x.edge, y.edge);
    if (x.transport === 'preserved') {
      assert.equal(y.transport, 'preserved'); roles.pair(native.tokens(x.node), actual.tokens(y.node), 'complete original preserved import/export');
      assert.deepEqual(actual.owner(y.node.end), native.owner(x.node.end), 'exact original preserved acquisition endpoint');
    } else if (x.edge.star) {
      if (n.item.format === 'system') {
        assert.equal(y.transport, 'setter');
        if (a.blue) {
          const last = inlineStar(a, y); actual.noSource(last.getStart(actual.source), last.end, 'complete generated System inline star forwarding');
        }
      } else {
        const owner = sourcePoint(x.edge.node.getStart(module.source));
        uniform(native, x.node, owner, 'complete Native original star publication');
        uniform(actual, y.node, owner, 'complete original star acquisition');
        const last = y.transport === 'inline-star' ? inlineStar(a, y) : y.node;
        if (last !== y.node) {
          uniform(actual, last, owner, 'complete original inline star publication');
          for (let offset = y.node.end; offset < last.getStart(actual.source); offset++) {
            assert.deepEqual(actual.owner(offset), owner, 'complete inline star acquisition/publication gap'); actual.cover(offset, offset + 1);
          }
        }
        assert.deepEqual(actual.owner(last.end), native.owner(x.node.end), 'exact original star publication endpoint');
      }
    } else if (['amd-wrapper', 'amd-parameter', 'setter'].includes(x.transport)) {
      if (x.node) native.noSource(x.node.getStart(native.source), x.node.end, 'complete generated import transport');
      if (y.node) actual.noSource(y.node.getStart(actual.source), y.node.end, 'complete generated import transport');
    } else {
      const owner = sourcePoint(x.edge.node.getStart(module.source));
      uniform(native, x.node, owner, 'complete Native original require acquisition');
      uniform(actual, y.node, owner, 'complete original require acquisition');
      assert.deepEqual(actual.owner(y.node.end), native.owner(x.node.end), 'exact original require acquisition endpoint');
    }
    acquisitionPairs.push({original: x.edge, native: x.node, actual: y.node});
  }
  for (const model of models) {
    if (model.item.format === 'system') systemTransport.read(ts, model.model, model.blue, model.consumed);
    publications.read(ts, model.model, model.blue, model.consumed);
    for (const node of model.item.sourceNodes) assert(model.consumed.has(node),
      'unclassified complete module statement: ' + node.getText(model.view.source).slice(0, 100));
  }
  roles.finish();
  return {native: n, actual: a, declarations: proof, acquisitionPairs, erasedImportComments: comments.removed, completeModuleValidated: true};
}

module.exports = {compare};
