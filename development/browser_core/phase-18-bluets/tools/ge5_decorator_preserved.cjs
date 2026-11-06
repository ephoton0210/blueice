// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete retained modules: no source token, trivia owner or transport is omitted.
const assert = require('node:assert/strict');
const {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
function comments(ts, view) {
  const scanner = ts.createScanner(ts.ScriptTarget.Latest, false, ts.LanguageVariant.Standard, view.code), result = [];
  for (let kind = scanner.scan(); kind !== ts.SyntaxKind.EndOfFileToken; kind = scanner.scan()) {
    if ([ts.SyntaxKind.SingleLineCommentTrivia, ts.SyntaxKind.MultiLineCommentTrivia].includes(kind)) {
      result.push({kind, text: scanner.getTokenText(), start: scanner.getTokenPos(), end: scanner.getTextPos()});
    }
  }
  return result;
}
function compare(ts, native, actual) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata, 'complete metadata/source/name tables');
  let previous = null;
  for (const point of actual.decoded.segments) {
    assert(!previous || point[0] > previous[0] || point[0] === previous[0] && point[1] > previous[1],
      'actual compiler segments cannot hide a zero-width source grant');
    previous = point;
  }
  const nc = comments(ts, native), ac = comments(ts, actual);
  function split(view, values) {
    const source = [], generated = [];
    for (const comment of values) {
      if (comment.text.startsWith('//# sourceMappingURL=')) {
        const payload = comment.text.slice('//# sourceMappingURL='.length);
        if (payload.startsWith('data:application/json;base64,')) {
          assert.equal(Buffer.from(payload.slice('data:application/json;base64,'.length), 'base64').toString('utf8'), view.raw,
            'inline publication carries this exact observed map');
        }
        view.noSource(comment.start, comment.end, 'generated map-publication comment'); generated.push(comment);
      } else source.push(comment);
    }
    assert.equal(generated.length, 1, 'one complete source-map publication');
    return {source, generated};
  }
  const n = split(native, nc), a = split(actual, ac);
  assert.equal(a.source.length, n.source.length, 'complete retained source-comment inventory');
  for (let index = 0; index < n.source.length; index++) {
    const x = n.source[index], y = a.source[index];
    assert.equal(y.kind, x.kind); assert.equal(y.text, x.text, 'complete source-comment UTF-16 payload');
    for (let at = 0; at < x.end - x.start; at++) assert.deepEqual(actual.owner(y.start + at), native.owner(x.start + at), 'source-comment character owner');
    assert.deepEqual(actual.owner(y.end), native.owner(x.end), 'source-comment endpoint');
    native.cover(x.start, x.end); actual.cover(y.start, y.end);
  }
  if (!n.generated[0].text.includes('base64,')) assert.equal(a.generated[0].text, n.generated[0].text, 'exact external map-publication identity');
  const codeBody = (view, comment) => view.code.slice(0, comment.start) + view.code.slice(comment.end);
  if (codeBody(native, n.generated[0]) === codeBody(actual, a.generated[0])) {
    assert.deepEqual(actual.decoded, native.decoded, 'identical JavaScript requires every decoded entry in order');
  }
  function roles(view, comments) {
    return view.tokens(view.source).concat(comments.map(comment => ({...comment,
      owners: Array.from({length: comment.end - comment.start}, (_, unit) => view.owner(comment.start + unit))})))
      .sort((left, right) => left.start - right.start);
  }
  const wanted = roles(native, n.source), observed = roles(actual, a.source);
  function declarations(view) {
    const nodes = [];
    function visit(node) { if (ts.isClassDeclaration(node)) nodes.push(node); ts.forEachChild(node, visit); }
    visit(view.source); return nodes;
  }
  const nativeClasses = declarations(native), actualClasses = declarations(actual), classEndpoints = [];
  assert.equal(actualClasses.length, nativeClasses.length, 'complete retained class declaration inventory');
  for (let index = 0; index < nativeClasses.length; index++) {
    const n = nativeClasses[index], a = actualClasses[index];
    assert.equal(a.name?.text, n.name?.text, 'same retained original class declaration');
    assert.deepEqual(actual.owner(a.end), native.owner(n.end), 'exact retained class declaration endpoint');
    classEndpoints.push({native: n, actual: a});
  }
  compareTokens(native, wanted, actual, observed, 'complete retained decorator module');
  compareGap(native, 0, wanted[0].start, actual, 0, observed[0].start, 'complete leading module gap');
  compareGap(native, wanted.at(-1).end, native.code.length, actual, observed.at(-1).end, actual.code.length, 'complete trailing module gap');
  assert.deepEqual(actual.owner(actual.code.length), native.owner(native.code.length), 'complete module endpoint');
  native.complete('every Native module UTF-16 character'); actual.complete('every Blue module UTF-16 character');
  return {nativeTokens: wanted, actualTokens: observed, nativeComments: n, actualComments: a,
    tokenPairs: wanted.length, classEndpoints, allCharactersClassified: true};
}
module.exports = {compare, comments};
