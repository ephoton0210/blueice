// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Plain original formals relocate into one generator callback with a closed receiver/arguments ABI.
const assert = require('node:assert/strict'), ir = require('./ge5_safe_home_ir.cjs'), pairs = require('./ge5_safe_home_pairs.cjs');
function compare(ts, n, nf, a, af, original, options, contract, {privateCallable = false} = {}) {
  assert.equal(options.target, 'ES2015');
  assert(original.modifiers?.some(m => m.kind === ts.SyntaxKind.AsyncKeyword) && !original.asteriskToken);
  const effective = original.parameters.filter(p => p.name.getText(n.cap.original) !== 'this');
  assert(effective.every(p => ts.isIdentifier(p.name) && !p.initializer && !p.dotDotDotToken));
  assert(!ir.walk(ts, original.body, ts.isParenthesizedExpression).length, 'source grouping is not projected as generated await precedence');
  function read(p, method) {
    const calls = ir.walk(ts, method.body, node => ts.isCallExpression(node) && ts.isIdentifier(node.expression) && p.aliases.get(node.expression.text) === '$helper.async');
    assert.equal(calls.length, 1, 'one exact driver invocation for this original async callable'); const driver = calls[0], callback = driver.arguments.at(-1);
    assert(ts.isFunctionExpression(callback) && callback.asteriskToken && !callback.name && !callback.modifiers?.length && ts.isBlock(callback.body));
    assert.equal(driver.arguments.length, p.blue ? 3 : 4); assert.equal(driver.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
    const isVoid = node => ts.isVoidExpression(node) && ts.isNumericLiteral(node.expression) && node.expression.text === '0';
    if (!p.blue) assert(isVoid(driver.arguments[1]) && isVoid(driver.arguments[2]), 'Native driver uses lexical source formals and the intrinsic Promise');
    else assert(ts.isIdentifier(driver.arguments[1]) && driver.arguments[1].text === 'arguments', 'all actual original arguments reach the relocated callback');
    let entry = method, returned = driver.parent;
    assert(ts.isReturnStatement(returned) && returned.expression === driver);
    if (p.blue && !privateCallable) {
      assert.equal(method.body.statements.length, 1); const outerReturn = method.body.statements[0];
      assert(ts.isReturnStatement(outerReturn) && ts.isCallExpression(outerReturn.expression));
      const apply = outerReturn.expression; assert(ts.isPropertyAccessExpression(apply.expression) && apply.expression.name.text === 'apply');
      assert.equal(apply.arguments.length, 2); assert.equal(apply.arguments[0].kind, ts.SyntaxKind.ThisKeyword); assert(ts.isIdentifier(apply.arguments[1]) && apply.arguments[1].text === 'arguments');
      entry = ir.unwrap(ts, apply.expression.expression); assert(ts.isFunctionExpression(entry) && /^__blueice_target_async_callable_\d+$/.test(entry.name?.text) && !entry.parameters.length && entry.body === returned.parent);
    }
    assert.equal(entry.body.statements.length, p.blue ? 2 : 1);
    if (p.blue) {
      const guard = entry.body.statements[0]; assert(ts.isIfStatement(guard) && !guard.elseStatement);
      assert(ts.isMetaProperty(guard.expression) && guard.expression.keywordToken === ts.SyntaxKind.NewKeyword && guard.expression.name.text === 'target');
      assert(ts.isThrowStatement(guard.thenStatement)); const error = guard.thenStatement.expression;
      assert(ts.isNewExpression(error) && ts.isIdentifier(error.expression) && error.expression.text === 'TypeError' && error.arguments.length === 1 && ts.isStringLiteral(error.arguments[0]) && error.arguments[0].text === 'Async function is not a constructor');
    }
    assert.equal(method.parameters.length, effective.length); assert.equal(callback.parameters.length, p.blue ? effective.length : 0);
    for (let index = 0; index < effective.length; index++) {
      const formal = p.blue ? callback.parameters[index] : method.parameters[index];
      assert(ts.isIdentifier(formal.name) && formal.name.text === effective[index].name.text && !formal.initializer && !formal.dotDotDotToken, 'exact original plain formal and source binding');
      if (p.blue) assert(ts.isIdentifier(method.parameters[index].name) && !ir.walk(ts, p.cap.original, node => ts.isIdentifier(node) && node.text === method.parameters[index].name.text).length, 'outer arity-only placeholder is fresh');
    }
    return {method, driver, callback, entry, returned};
  }
  const nn = read(n, nf), aa = read(a, af), nv = n.view, av = a.view, points = [];
  const nativeHeader = nv.tokens(nf).filter(t => t.end <= nf.body.getStart(nv.source) + 1);
  if (privateCallable) {
    assert(af.name); av.noSource(af.name.getStart(av.source), af.name.end, 'generated overwritten private async callable name');
    const fn = nv.tokens(nf).find(t => t.text === 'function'), actual = av.tokens(af).find(t => t.text === 'function');
    contract.pair([fn], [actual], 'exact private async callable function creation header');
  } else {
    contract.pair(nativeHeader, av.tokens(af).filter(t => t.end <= af.body.getStart(av.source) + 1), 'complete original public async method header');
  }
  const signature = nativeHeader.slice(nativeHeader.findIndex(t => t.text === '('));
  const callbackHeader = av.tokens(aa.callback).filter(t => t.end <= aa.callback.body.getStart(av.source) + 1), callbackOpen = callbackHeader.findIndex(t => t.text === '(');
  assert(callbackOpen > 0); contract.pair(signature, callbackHeader.slice(callbackOpen), 'complete relocated original safe-home async signature');
  av.noSource(aa.callback.getStart(av.source), callbackHeader[callbackOpen].start, 'generated safe-home callback generator keyword');
  for (let index = 0; index < effective.length; index++) points.push(...pairs.ends(ts, nv, nf.parameters[index], av, aa.callback.parameters[index], 'exact relocated safe-home formal'));
  const groups = {native: [], actual: []};
  function bodyTokens(p, item, side) {
    for (const group of ir.walk(ts, item.callback.body, ts.isParenthesizedExpression)) {
      assert(ts.isYieldExpression(ir.unwrap(ts, group)) || ts.isYieldExpression(group.parent), 'only independently identified compiler await precedence groups are projected');
      const delimiters = group.getChildren(p.view.source).filter(ts.isToken); assert.equal(delimiters.length, 2);
      for (const token of delimiters) {
        groups[side].push(token);
        if (p.blue) p.view.noSource(token.getStart(p.view.source), token.end, 'generated exact await precedence delimiter');
        else p.view.cover(token.getStart(p.view.source), token.end);
      }
    }
    return pairs.tokens(ts, p, item.callback.body).filter(t => t.start > item.callback.body.getStart(p.view.source) && t.end < item.callback.body.end && !groups[side].some(g => g.getStart(p.view.source) === t.start));
  }
  const nt = bodyTokens(n, nn, 'native'), at = bodyTokens(a, aa, 'actual'); contract.pair(nt, at, 'complete source safe-home async body and all private-operation operands');
  const awaitCount = ir.walk(ts, original.body, ts.isAwaitExpression).length;
  assert.equal(ir.walk(ts, nn.callback.body, ts.isYieldExpression).length, awaitCount); assert.equal(ir.walk(ts, aa.callback.body, ts.isYieldExpression).length, awaitCount);
  function ends(left, right) {
    const ln = ir.unwrap(ts, left), an = ir.unwrap(ts, right); assert.equal(an.kind, ln.kind, 'complete source async logical subtree kind');
    const actualEnd = ts.isYieldExpression(an) ? right.end : an.end;
    assert.deepEqual(av.owner(actualEnd), nv.owner(left.end), 'exact complete source async logical endpoint '+ts.SyntaxKind[ln.kind]+' '+left.getText(nv.source)+' / '+right.getText(av.source));
    points.push({kind:ts.SyntaxKind[ln.kind],native:left.end,actual:actualEnd,owner:nv.owner(left.end)});
    const nc = [], ac = []; ts.forEachChild(ln, child => {nc.push(child);}); ts.forEachChild(an, child => {ac.push(child);}); assert.equal(ac.length,nc.length);
    for(let i=0;i<nc.length;i++) ends(nc[i],ac[i]);
  }
  ends(nn.callback.body, aa.callback.body);
  // Native transport is fixed by the authoritative four-argument awaiter. Actual
  // unpaired transport is generated except the independently proved body braces.
  const actualCloses = new Set([af.body.end - 1, aa.entry.body.end - 1, aa.callback.body.end - 1]);
  for (const token of av.tokens(af)) if (!av.covered(token.start)) {
    if (actualCloses.has(token.start)) pairs.role(av, token.start, token.end, nv.owner(token.start === aa.callback.body.end - 1 ? nn.callback.body.end - 1 : nf.body.end - 1), 'complete safe-home async frame source completion');
    else if (aa.callback.end <= token.start && token.end <= aa.returned.end) pairs.role(av, token.start, token.end, nv.owner(nn.callback.end), 'exact safe-home awaiter completion carrier');
    else if (token.start === af.body.getStart(av.source)) pairs.role(av, token.start, token.end, nv.owner(nf.body.getStart(nv.source)), 'safe-home callable body entry role');
    else av.noSource(token.start, token.end, 'complete generated safe-home async driver and arity transport');
  }
  for (const token of nv.tokens(nf)) if (!nv.covered(token.start)) pairs.role(nv, token.start, token.end, nv.owner(token.start), 'complete Native safe-home async transport token role');
  assert.deepEqual(av.owner(aa.returned.end), nv.owner(nn.returned.end), 'exact complete safe-home driver return endpoint');
  points.push({kind:'AsyncDriverReturn',native:nn.returned.end,actual:aa.returned.end,owner:nv.owner(nn.returned.end)});
  assert.deepEqual(av.owner(af.end), nv.owner(nf.end), 'complete safe-home async callable logical endpoint');
  assert.deepEqual(av.owner(aa.callback.end), nv.owner(nn.callback.end), 'complete safe-home async callback logical endpoint');
  return {source: original, native: nn, actual: aa, points};
}
module.exports = {compare};
