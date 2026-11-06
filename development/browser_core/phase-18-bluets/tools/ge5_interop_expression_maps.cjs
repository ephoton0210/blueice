// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Typed semantic pairs preserve every expression token and exact logical end.
const assert = require('node:assert/strict');
const imports = require('./ge5_interop_imports.cjs'), references = require('./ge5_interop_references.cjs');

function compare(ts, native, nativeBinding, actual, actualBinding, original, nn, an, accounting) {
  assert.equal(imports.checked(nativeBinding, native).module, imports.checked(actualBinding, actual).module,
    'both projections use the exact independently owned original module');
  const wanted = references.compare(ts, nativeBinding, native, original, nn);
  const observed = references.compare(ts, actualBinding, actual, original, an);
  assert.equal(wanted.length, observed.length, 'complete semantic expression inventory');
  function uniform(view, start, end, owner, label) {
    for (let offset = start; offset < end; offset++) assert.deepEqual(view.owner(offset), owner, label);
    view.cover(start, end); view.roleStarts.set(start, owner); view.roleEnds.set(end, view.owner(end));
  }
  function publication(view, pair) {
    if (!ts.isBinaryExpression(pair.original) || !pair.transport || !ts.isCallExpression(pair.transport)) return;
    const call = pair.transport, inner = call.arguments[1];
    assert(ts.isBinaryExpression(inner));
    uniform(view, call.getStart(view.source), inner.getStart(view.source), view.owner(inner.left.getStart(view.source)),
      'complete original System publication prefix');
    uniform(view, inner.end, call.end, view.owner(inner.end), 'complete original System publication suffix');
  }
  const points = [];
  for (let index = 0; index < wanted.length; index++) {
    const n = wanted[index], a = observed[index];
    assert.equal(n.original, a.original); assert.equal(n.role, a.role);
    assert.deepEqual(actual.owner(a.actual.end), native.owner(n.actual.end), 'exact interop expression logical endpoint');
    points.push(Object.freeze({original: n.original, native: n.actual, actual: a.actual, role: n.role}));
    publication(native, n); publication(actual, a);
    if (ts.isIdentifier(n.original) || ts.isNumericLiteral(n.original) || ts.isStringLiteral(n.original)) {
      const ns = n.actual.getStart(native.source), as = a.actual.getStart(actual.source), owner = native.owner(ns);
      // Name spelling may differ only after the independent binding comparison.
      // A transformed identifier has one original phase across its whole span.
      uniform(native, ns, n.actual.end, owner, 'complete Native source atom');
      uniform(actual, as, a.actual.end, owner, 'complete source atom UTF-16 interior');
      actual.nativeStarts.set(as, ns); actual.nativeEnds.set(a.actual.end, n.actual.end);
    } else {
      const nt = native.tokens(n.actual).filter(token => !native.covered(token.start));
      const at = actual.tokens(a.actual).filter(token => !actual.covered(token.start));
      accounting.pair(nt, at, 'complete interop expression syntax');
    }
  }
  return Object.freeze({pairs: Object.freeze(points), completeModuleValidated: false});
}

module.exports = {compare};
