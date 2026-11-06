// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// All source module roles compose with the complete switch/delegation machine.
const assert = require('node:assert/strict'), envelope = require('./ge5_module_envelope.cjs');
const source = require('./ge5_generator_switch_source.cjs'), helpers = require('./ge5_generator_switch_helpers.cjs');
const {compareTokens, compareGap} = require('./ge5_role_stream.cjs');
function compare(ts, native, actual, options, original) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
  if (native.code === actual.code) assert.deepEqual(actual.decoded, native.decoded);
  const origin = source.read(ts, original), lowered = options.target === 'ES5';
  const dependencies = lowered && options.importHelpers ? ['tslib'] : [];
  const n = envelope.read(ts, native, options, {originals: [origin.file], dependencies});
  const a = envelope.read(ts, actual, options, {blue: true, originals: [origin.file], dependencies});
  n.blue = false; a.blue = true; n.classifyTransport(); a.classifyTransport();
  const nc = helpers.create(ts, n, options, origin), ac = helpers.create(ts, a, options, origin);
  const np = helpers.checked(nc, native), ap = helpers.checked(ac, actual);
  const fnName = origin.fn.name.text, iteratorName = origin.binding.name.text, resultName = origin.exported.name.text;
  const start = (view, node) => node.getStart(view.source);
  function uniform(view, from, to, owner, label) {
    for (let at = from; at < to; at++) assert.deepEqual(view.owner(at), owner, label + ' at ' + at);
    view.cover(from, to);
  }
  for (const view of [native, actual]) {
    const lines = original.split('\n'), seen = new Map();
    assert.equal(view.decoded.metadata.sources.length, 1);
    for (const segment of view.decoded.segments) {
      if (segment.length > 2) assert(segment[2] === 0 && segment[3] < lines.length && segment[4] <= lines[segment[3]].length,
        'every callable source coordinate is within original UTF-16 content');
      const key = segment[0] + ':' + segment[1];
      if (seen.has(key)) assert.deepEqual(segment.slice(2), seen.get(key), 'no conflicting same-position source transition');
      seen.set(key, segment.slice(2));
    }
    const publication = /(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(view.code); assert(publication);
    if (options.inlineSourceMap) assert.equal(Buffer.from(publication[1].split('base64,')[1], 'base64').toString(), view.raw);
    else assert.equal(publication[1], /(?:^|\n)\/\/# sourceMappingURL=([^\r\n]+)\r?\n?$/.exec(native.code)[1]);
  }
  function read(item, proof) {
    const view = item.view, roles = new Map(), generated = [...proof.generated], hoisted = [];
    function add(key, node) {assert(!roles.has(key), 'unique complete module role ' + key); roles.set(key, node);}
    function scaffold(node, label) {view.noSource(start(view, node), node.end, label); generated.push(node);}
    for (const node of proof.remaining) {
      if (node === proof.fn) {add('generator', node); continue;}
      if (ts.isVariableStatement(node)) {
        const declarations = node.declarationList.declarations;
        if (declarations.every(d => ts.isIdentifier(d.name) && !d.initializer)) {
          for (const declaration of declarations) hoisted.push(declaration.name.text);
          scaffold(node, 'complete relocated module binding inventory'); continue;
        }
        if (declarations.length === 1) {
          const name = declarations[0].name.getText(view.source);
          if (name === '__moduleName' && item.format === 'system' && !item.blue) {
            item.exact(node, `var __moduleName=${item.system.context}&&${item.system.context}.id;`);
            scaffold(node, 'complete System module identity'); continue;
          }
          if (name === iteratorName || name === resultName) {add(name === iteratorName ? 'iterator' : 'result', node); continue;}
        }
      }
      assert(ts.isExpressionStatement(node), 'complete emitted source statement inventory');
      const expression = node.expression;
      if (ts.isStringLiteral(expression)) {item.exact(node, `'use strict';`); scaffold(node, 'module directive'); continue;}
      if (ts.isCallExpression(expression)) {
        const callee = expression.expression.getText(view.source);
        if (callee === 'Object.defineProperty') {
          item.exact(node, 'Object.defineProperty(exports,"__esModule",{value:true});'); scaffold(node, 'module identity marker'); continue;
        }
        if (item.format === 'system') {
          if (!item.blue && callee === item.system.publisher) {
            assert.equal(expression.arguments.length, 2); assert(ts.isStringLiteral(expression.arguments[0]));
            if (expression.arguments[0].text === fnName) {
              item.exact(node, `${callee}(${JSON.stringify(fnName)},${fnName});`); scaffold(node, 'hoisted generator publication'); continue;
            }
            assert.equal(expression.arguments[0].text, resultName); add('result', node); continue;
          }
          if (item.blue && callee === item.system.stem + '_slot') {
            assert.equal(expression.arguments.length, 1); assert(ts.isStringLiteral(expression.arguments[0]));
            assert([fnName, resultName].includes(expression.arguments[0].text));
            item.exact(node, `${callee}(${JSON.stringify(expression.arguments[0].text)});`); scaffold(node, 'System export slot'); continue;
          }
          if (item.blue && callee === item.system.stem + '_refresh') {
            item.exact(node, `${callee}();`); scaffold(node, 'System export refresh'); continue;
          }
        }
      }
      if (ts.isBinaryExpression(expression) && expression.operatorToken.kind === ts.SyntaxKind.EqualsToken) {
        const name = expression.left.getText(view.source);
        if (name === iteratorName) {assert.equal(item.format, 'system'); add('iterator', node); continue;}
        if (name === 'exports.' + resultName) {
          if (ts.isVoidExpression(expression.right)) {
            item.exact(node, `exports.${resultName}=void 0;`); scaffold(node, 'export initialization');
          } else add('result', node);
          continue;
        }
        if (name === 'exports.' + fnName) {
          item.exact(node, `exports.${fnName}=${fnName};`); add('publication', node); continue;
        }
        if (item.format === 'system' && item.blue && ts.isElementAccessExpression(expression.left)
          && expression.left.expression.getText(view.source) === 'exports'
          && ts.isStringLiteral(expression.left.argumentExpression) && expression.left.argumentExpression.text === fnName) {
          item.exact(node, `exports[${JSON.stringify(fnName)}]=${fnName};`); scaffold(node, 'System generator publication'); continue;
        }
      }
      assert.fail('unclassified module statement ' + node.getText(view.source));
    }
    assert.deepEqual(hoisted.sort(), (item.format === 'system'
      ? [iteratorName, ...!item.blue ? [resultName] : [], ...proof.namespace ? [proof.namespace] : []] : []).sort());
    const published = item.format !== 'system' && !view.tokens(proof.fn).some(t => t.text === 'export');
    assert.deepEqual([...roles.keys()].sort(), ['generator', 'iterator', 'result', ...published ? ['publication'] : []].sort());
    return {roles, generated};
  }
  const nn = read(n, np), aa = read(a, ap);
  const machine = lowered ? require('./ge5_generator_switch_frames.cjs').compare(ts, native, np, actual, ap, origin,
    {native: nc, actual: ac}) : null;
  if (!lowered) compareTokens(native, native.tokens(np.fn), actual, actual.tokens(ap.fn), 'complete retained generator');
  function canonical(item, node) {
    const view = item.view; let tokens = view.tokens(node);
    if (item.format === 'system' && !item.blue && ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
      && node.expression.expression.getText(view.source) === item.system.publisher) {
      const call = node.expression, assignment = call.arguments[1];
      assert(ts.isBinaryExpression(assignment) && ts.isIdentifier(assignment.left) && assignment.left.text === resultName);
      const begin = start(view, call), binding = start(view, assignment.left), owner = view.owner(begin);
      uniform(view, begin, binding, owner, 'Native System publication prefix');
      tokens = tokens.filter(t => binding <= t.start && t.end <= assignment.end || t.text === ';' && t.start >= call.end);
      tokens.unshift({text: 'exports', start: begin, end: binding, owners: Array(7).fill(owner)},
        {text: '.', start: binding, end: binding, owners: [owner]});
      uniform(view, assignment.end, call.end, view.owner(assignment.end), 'Native System publication ending');
    }
    return tokens;
  }
  const roles = new Map();
  for (const [key, node] of nn.roles) {
    const peer = aa.roles.get(key); assert(peer); roles.set(key, {native: node, actual: peer});
    if (key !== 'generator') compareTokens(native, canonical(n, node), actual, canonical(a, peer), 'whole module ' + key);
  }
  const order = view => [...roles].sort((x, y) => start(view, x[1][view === native ? 'native' : 'actual'])
    - start(view, y[1][view === native ? 'native' : 'actual'])).map(([key]) => key);
  const no = order(native); assert.deepEqual(order(actual), no, 'complete ordered module roles');
  const ignored = aa.generated.map(node => [start(actual, node), node.end]);
  for (let index = 0; index < no.length; index++) {
    const current = roles.get(no[index]), previous = index ? roles.get(no[index - 1]) : null;
    compareGap(native, previous ? previous.native.end : 0, start(native, current.native),
      actual, previous ? previous.actual.end : 0, start(actual, current.actual), 'ordered module boundary ' + index, ignored);
  }
  const last = roles.get(no.at(-1));
  compareGap(native, last.native.end, native.code.length, actual, last.actual.end, actual.code.length, 'complete final module gap', ignored);
  const comments = (ts.getLeadingCommentRanges(original, 0) || []).map(c => original.slice(c.pos, c.end));
  for (const view of [native, actual]) {
    assert.deepEqual(view.comments().filter(c => comments.includes(c.text)).map(c => c.text), comments);
    for (const comment of view.comments()) if (!comments.includes(comment.text)) {
      if (view === native && lowered && comment.text.startsWith('/*') && start(view, np.fn) <= comment.start && comment.end <= np.fn.end) {
        for (let at = comment.start; at < comment.end; at++) assert(view.covered(at), 'independently covered Native operation comment');
      } else view.noSource(comment.start, comment.end, 'complete generated protocol/publication comment');
    }
    for (let at = 0; at < view.code.length; at++) if (!view.covered(at)) {
      assert(/\s/.test(view.code[at]), 'unclassified source syntax ' + at + ' ' + view.code.slice(at, at + 50));
      assert.equal(view.owner(at), null, 'unclassified source grant ' + at); view.cover(at, at + 1);
    }
    view.complete('whole generator switch/delegation module source-role/generated-gap closure');
  }
  assert.deepEqual(actual.owner(actual.code.length), native.owner(native.code.length));
  return {origin, native: n, actual: a, roles, machine, helpers: {native: nc, actual: ac}};
}
module.exports = {compare};
