// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Method roles retain complete source tokens and separately proved transports.
const assert = require('node:assert/strict');
const {inspect} = require('./ge5_async_method_ir.cjs');
const {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
function authority(native, actual, options, original) {
  const lines = original.split('\n');
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
  for (const view of [native, actual]) {
    assert.equal(view.decoded.metadata.sources.length, 1);
    for (const segment of view.decoded.segments) if (segment.length > 2) {
      assert(segment[2] === 0 && segment[3] < lines.length && segment[4] <= lines[segment[3]].length,
        'every original method coordinate is bounded, including shadowed entries');
    }
    const match = /(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(view.code); assert(match);
    if (options.inlineSourceMap) {
      assert(match[1].startsWith('data:application/json;base64,'));
      assert.equal(Buffer.from(match[1].slice('data:application/json;base64,'.length), 'base64').toString(), view.raw);
    } else assert.equal(match[1], /(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(native.code)[1]);
  }
  if (native.code === actual.code) assert.deepEqual(actual.decoded, native.decoded);
}
function finish(ts, native, n, actual, a, original) {
  const start = (view, node) => node.getStart(view.source);
  compareTokens(native, native.tokens(n.result), actual, actual.tokens(a.result), 'complete method result publication');
  compareGap(native, 0, start(native, n.boxStatement), actual, 0, start(actual, a.boxStatement), 'method module prefix');
  compareGap(native, n.boxStatement.end, start(native, n.result), actual, a.boxStatement.end, start(actual, a.result), 'method object/publication boundary');
  compareGap(native, n.result.end, native.code.length, actual, a.result.end, actual.code.length, 'method module ending');
  const comments = [], scanner = ts.createScanner(ts.ScriptTarget.Latest, false, ts.LanguageVariant.Standard, original);
  for (let kind = scanner.scan(); kind !== ts.SyntaxKind.EndOfFileToken; kind = scanner.scan()) {
    if ([ts.SyntaxKind.SingleLineCommentTrivia, ts.SyntaxKind.MultiLineCommentTrivia].includes(kind)) comments.push(scanner.getTokenText());
  }
  for (const view of [native, actual]) {
    assert.deepEqual(view.comments().filter(comment => comments.includes(comment.text)).map(comment => comment.text), comments);
    for (const comment of view.comments()) if (!comments.includes(comment.text)) {
      const covered = Array.from({length: comment.end - comment.start}, (_, unit) => view.covered(comment.start + unit)).every(Boolean);
      if (!covered) view.noSource(comment.start, comment.end, 'complete generated method comment');
    }
    for (let offset = 0; offset < view.code.length; offset++) if (!view.covered(offset)) {
      assert(/\s/.test(view.code[offset]), 'unclassified method syntax at ' + offset);
      assert.equal(view.owner(offset), null, 'unclassified method source grant at ' + offset);
      view.cover(offset, offset + 1);
    }
    view.complete('complete method source-role/generated-gap contract');
  }
  assert.deepEqual(actual.owner(actual.code.length), native.owner(native.code.length));
}
function compareRetained(ts, native, actual, options, original) {
  authority(native, actual, options, original);
  const n = inspect(ts, native, options, original, false), a = inspect(ts, actual, options, original, true);
  assert(n.retained && a.retained);
  compareTokens(native, native.tokens(n.boxStatement), actual, actual.tokens(a.boxStatement), 'complete retained method object');
  require('./ge5_async_operand_endpoints.cjs').compare(ts, native, n, actual, a, options);
  finish(ts, native, n, actual, a, original);
  return {native: n, actual: a};
}
function compare(ts, native, actual, options, original) {
  if (!['ES5', 'ES2015', 'ES2016'].includes(options.target)) return compareRetained(ts, native, actual, options, original);
  authority(native, actual, options, original);
  const n = inspect(ts, native, options, original, false), a = inspect(ts, actual, options, original, true);
  const cap = require('./ge5_async_method_frames.cjs').frames(ts, native, n, actual, a);
  if (options.target === 'ES5') {
    const machine = require('./ge5_continuation_machine.cjs');
    const nm = machine.read(ts, native, n.callback, 'native', [original]), am = machine.read(ts, actual, a.callback, 'blue', [original]);
    nm.reviewedSignature = cap;
    require('./ge5_linear_roles.cjs').compareFunction(ts, native, nm, actual, am);
  } else require('./ge5_async_method_body.cjs').compare(ts, native, n, actual, a);
  require('./ge5_async_operand_endpoints.cjs').compare(ts, native, n, actual, a, options);
  finish(ts, native, n, actual, a, original);
  return {native: n, actual: a};
}
module.exports = {authority, finish, compareRetained, compare};
