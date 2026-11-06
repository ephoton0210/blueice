// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A runtime dependency parameter/setter can erase its original attached comment.
const assert = require('node:assert/strict'), sources = require('./ge5_interop_source.cjs');

function project(ts, original, filename, options, native, actual) {
  const module = sources.checked(original).modules[filename], removed = []; assert(module);
  if (!['amd', 'system'].includes(String(options.module).toLowerCase())) return {text: module.source.text, removed};
  for (const edge of module.imports) {
    if (!ts.isImportDeclaration(edge.node) || edge.node.importClause?.isTypeOnly) continue;
    const node = edge.node, source = module.source;
    const comments = (ts.getLeadingCommentRanges(source.text, node.getFullStart()) || [])
      .filter(comment => node.getFullStart() <= comment.pos && comment.end <= node.getStart(source));
    let attached = 0;
    for (let index = 0; index < comments.length; index++) {
      const next = comments[index + 1]?.pos ?? node.getStart(source);
      if ((source.text.slice(comments[index].end, next).match(/\n/g) || []).length >= 2) attached = index + 1;
    }
    for (const comment of comments.slice(attached)) {
      const text = source.text.slice(comment.pos, comment.end);
      assert(!native.comments().some(item => item.text === text), 'Native must erase this exact attached runtime-import comment');
      assert(!actual.comments().some(item => item.text === text), 'elided runtime import cannot retain its attached source comment');
      for (const segment of native.decoded.segments.filter(point => point.length > 2)) {
        assert.equal(segment[2], 0);
        const offset = source.getPositionOfLineAndCharacter(segment[3], segment[4]);
        assert(offset < node.getStart(source) || offset >= node.end, 'erased runtime import has no Native emitted source role');
      }
      removed.push({start: comment.pos, end: comment.end, text, original: node});
    }
  }
  let text = module.source.text;
  for (const comment of removed.toReversed()) text = text.slice(0, comment.start)
    + text.slice(comment.start, comment.end).replace(/[^\r\n]/g, ' ') + text.slice(comment.end);
  assert.equal(text.length, module.source.text.length, 'comment inventory projection preserves original UTF-16 positions');
  return {text, removed};
}

module.exports = {project};
