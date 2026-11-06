// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Token characters and physical gaps retain their original generated offsets.
const assert = require('node:assert/strict'), maps = require('./ge5_maps.cjs');
function stream(ts, code, raw) {
  const source = ts.createSourceFile('emitted.js', code, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  assert.deepEqual(source.parseDiagnostics.map(item => item.code), [], 'emitted JavaScript syntax');
  const decoded = maps.decode(raw, code), lines = new Map();
  for (const segment of decoded.segments) {
    if (!lines.has(segment[0])) lines.set(segment[0], []);
    lines.get(segment[0]).push(segment);
  }
  function owner(offset) {
    const { line, character } = source.getLineAndCharacterOfPosition(offset), segments = lines.get(line) || [];
    let lo = 0, hi = segments.length;
    while (lo < hi) { const mid = (lo + hi) >>> 1; if (segments[mid][1] <= character) lo = mid + 1; else hi = mid; }
    const segment = lo ? segments[lo - 1] : null;
    return segment?.length > 2 ? segment.slice(2) : null;
  }
  function token(node) {
    const start = node.getStart(source), end = node.end;
    return { text: code.slice(start, end), start, end,
      owners: Array.from({ length: end - start }, (_, index) => owner(start + index)) };
  }
  function tokens(node) {
    const result = [];
    function visit(node) {
      if (node.kind >= ts.SyntaxKind.FirstJSDocNode && node.kind <= ts.SyntaxKind.LastJSDocNode) return;
      if (ts.isToken(node)) { if (node.kind !== ts.SyntaxKind.EndOfFileToken) result.push(token(node)); return; }
      for (const child of node.getChildren(source)) visit(child);
    }
    visit(node); return result;
  }
  const coverage = new Uint8Array(code.length);
  let commentCache;
  function comments() {
    if (commentCache) return commentCache;
    commentCache = [];
    const scanner = ts.createScanner(ts.ScriptTarget.Latest, false, ts.LanguageVariant.Standard, code);
    let previous = null;
    for (let kind = scanner.scan(); kind !== ts.SyntaxKind.EndOfFileToken; kind = scanner.scan()) {
      if (kind === ts.SyntaxKind.WhitespaceTrivia || kind === ts.SyntaxKind.NewLineTrivia) continue;
      if (previous) { previous.after = scanner.getTokenPos(); previous = null; }
      if (kind === ts.SyntaxKind.SingleLineCommentTrivia || kind === ts.SyntaxKind.MultiLineCommentTrivia) {
        previous = { start: scanner.getTokenPos(), end: scanner.getTextPos(), text: scanner.getTokenText(), after: code.length };
        commentCache.push(previous);
      }
    }
    return commentCache;
  }
  function cover(start, end) { coverage.fill(1, start, end); }
  function noSource(start, end, label) {
    for (let offset = start; offset < end; offset++) assert.equal(owner(offset), null, label + ': generated source grant at ' + offset);
    cover(start, end);
  }
  function complete(label) {
    const offset = coverage.indexOf(0);
    assert(offset < 0, label + ': unclassified UTF-16 character at ' + offset + ': ' + JSON.stringify(code.slice(offset, offset + 40)));
  }
  return { source, code, raw, decoded, owner, token, tokens, comments, noSource, cover, complete,
    covered: offset => coverage[offset] === 1 };
}

function orderedGap(native, from, to, actual, start, end, label, actualGenerated = []) {
  const allowed = [native.owner(from)];
  for (let offset = from; offset < to; offset++) {
    const value = native.owner(offset); if (JSON.stringify(allowed.at(-1)) !== JSON.stringify(value)) allowed.push(value);
  }
  allowed.push(native.owner(to));
  let cursor = 0;
  for (let offset = start; offset < end; offset++) {
    if (actualGenerated.some(([start, end]) => start <= offset && offset < end)) continue;
    const value = actual.owner(offset);
    if (value === null && (allowed.includes(null) || from === to || native.code.slice(from, to).includes('\n') || actual.code.slice(start, end).includes('\n'))) continue;
    const next = allowed.findIndex((candidate, at) => at >= cursor && JSON.stringify(candidate) === JSON.stringify(value));
    assert(next >= 0, label + ': ordered gap: ' + JSON.stringify(value)); cursor = next;
  }
  native.cover(from, to); actual.cover(start, end);
}

function compareGap(native, from, to, actual, start, end, label, actualGenerated = []) {
  // A mapped comment is an independent source role. Its earlier owner must
  // never remain admissible in a later gap merely because a larger interval
  // also contains that owner before the comment's logical ending.
  function within(view, first, last) {
    const comments = view.comments(); let lo = 0, hi = comments.length;
    while (lo < hi) { const mid = (lo + hi) >>> 1; if (comments[mid].start < first) lo = mid + 1; else hi = mid; }
    const result = [];
    for (let index = lo; index < comments.length && comments[index].start < last; index++) {
      if (comments[index].end <= last) result.push(comments[index]);
    }
    return result;
  }
  const wanted = within(native, from, to).filter(comment => native.owner(comment.start) !== null);
  if (!wanted.length) return orderedGap(native, from, to, actual, start, end, label, actualGenerated);
  const observed = within(actual, start, end);
  let cursor = start;
  for (const comment of wanted) {
    const counterpart = observed.find(item => item.start >= cursor && item.text === comment.text);
    if (!counterpart) continue; // Different lowered opcode comments have their own AST contracts.
    cursor = counterpart.end;
    for (let unit = 0; unit < comment.end - comment.start; unit++) {
      assert.deepEqual(actual.owner(counterpart.start + unit), native.owner(comment.start + unit), label + ': mapped comment UTF-16 interior');
    }
    const nextNative = Math.min(comment.after, to), nextActual = Math.min(counterpart.after, end);
    if (comment.end < nextNative && counterpart.end < nextActual) {
      assert.deepEqual(actual.owner(counterpart.end), native.owner(comment.end), label + ': mapped comment endpoint');
    }
    orderedGap(native, comment.end, nextNative, actual, counterpart.end, nextActual, label + ': after mapped comment');
  }
  orderedGap(native, from, to, actual, start, end, label, actualGenerated);
}

function compareTokens(native, wanted, actual, observed, label, actualGenerated = []) {
  assert.equal(observed.length, wanted.length, label + ': complete token inventory');
  for (let index = 0; index < wanted.length; index++) {
    const a = wanted[index], b = observed[index];
    assert.equal(b.text, a.text, label + ': token ' + index);
    assert.deepEqual(b.owners, a.owners, label + ': UTF-16 owners of token ' + index + ' ' + a.text);
    native.cover(a.start, a.end); actual.cover(b.start, b.end);
    if (index === 0) continue;
    const pa = wanted[index - 1], pb = observed[index - 1];
    if (pa.end > a.start || pb.end > b.start) continue; // separately classified relocated signature sections
    compareGap(native, pa.end, a.start, actual, pb.end, b.start, label + ': before token ' + index + ' ' + a.text, actualGenerated);
  }
}
module.exports = { stream, compareTokens, compareGap };
