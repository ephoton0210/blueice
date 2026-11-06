// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Decorator phases compose with independently proved class and module transport roles.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path');
const helper = require("./ge5_decorator_helpers.cjs");
const moduleContract = require("./ge5_class_module.cjs"), calls = require("./ge5_decorator_roles.cjs");
const rest = require("./ge5_decorator_rest.cjs"), {compareGap} = require("./ge5_role_stream.cjs");

// Bounded K.11 proposal: exact unchanged sources; existing frozen readers stay untouched.
const k11SourceHashes=new Set(["6b5ea6445580bfbe55cb491dde83154b7689ae38c71fdda2753fa8acb819aba4","84d4cad4d110aa257b3c00b90344e57390c4ddff460cf2997763b86cdea82d3f"]);
function validateK11(ts,options,text){
 assert(k11SourceHashes.has(require('node:crypto').createHash('sha256').update(text).digest('hex')),'exact original K11 legacy source');
 assert.equal(options.target,'ES2022');assert.equal(options.module,'ES2022');assert.equal(options.experimentalDecorators,true);
 const original=ts.createSourceFile('original.ts',text,ts.ScriptTarget.Latest,true);assert.equal(original.statements.filter(ts.isClassDeclaration).length,1,'complete original class inventory');
 for(const fn of original.statements.filter(ts.isFunctionDeclaration)){assert(fn.name&&fn.body&&!fn.asteriskToken&&!fn.modifiers?.some(m=>m.kind===ts.SyntaxKind.AsyncKeyword));assert(fn.parameters.every(p=>ts.isIdentifier(p.name)&&!p.dotDotDotToken&&!p.questionToken&&!p.initializer),'complete fixed source callable arity');}
}

function compare(ts, native, actual, options, originalText) {
  validateK11(ts,options,originalText);
  let phases;
  let previous = null;
  for (const point of actual.decoded.segments) {
    assert(!previous || point[0] > previous[0] || point[0] === previous[0] && point[1] > previous[1], 'ordered unique decoded segments cannot hide a zero-width source grant');
    previous = point;
  }
  const comments = require("./ge5_decorator_preserved.cjs").comments;
  const originalComments = comments(ts, {code: originalText}).map(comment => comment.text);
  const original = ts.createSourceFile('original.ts', originalText, ts.ScriptTarget.Latest, true);
  const sourceClassCount = original.statements.filter(ts.isClassDeclaration).length;
  const asset = fs.readFileSync(path.resolve(__dirname, '../../../../backend/bluets/src/emitter/targets/class_helpers.v1.js'), 'utf8');
  const assetComments = comments(ts, {code: asset}).map(comment => comment.text);
  const systemAsset = fs.readFileSync(path.resolve(__dirname, '../../../../backend/bluets/src/emitter/system_helpers.v1.js'), 'utf8');
  const systemComments = String(options.module).toLowerCase() === 'system' ? comments(ts, {code: systemAsset}).map(comment => comment.text) : [];
  for (const [view, blue] of [[native, false], [actual, true]]) {
    const all = comments(ts, view), publication = all.filter(comment => comment.text.startsWith('//# sourceMappingURL='));
    assert.equal(publication.length, 1, 'one exact complete map publication');
    const payload = publication[0].text.slice('//# sourceMappingURL='.length);
    if (blue && payload.startsWith('data:application/json;base64,')) assert.equal(Buffer.from(payload.split('base64,')[1], 'base64').toString('utf8'), actual.raw, 'inline publication carries this observed map');
    const generated = all.filter(comment => !originalComments.includes(comment.text) && comment !== publication[0]).map(comment => comment.text);
    const expected = options.target !== 'ES5' ? [] : blue ? options.importHelpers || options.noEmitHelpers ? [] : Array.from({length: sourceClassCount}, () => assetComments).flat() : Array(sourceClassCount).fill('/** @class */');
    if (blue) expected.unshift(...systemComments);
    assert.deepEqual(generated, expected, 'complete AST-derived class/helper generated comment inventory');
  }
  const np = comments(ts, native).find(comment => comment.text.startsWith('//# sourceMappingURL=')), ap = comments(ts, actual).find(comment => comment.text.startsWith('//# sourceMappingURL='));
  if (!np.text.includes('base64,')) assert.equal(ap.text, np.text, 'exact external map publication identity');
  function covered(view, node) { for (let offset = node.getStart(view.source); offset < node.end; offset++) if (!view.covered(offset)) return false; return true; }
  function pairUnits(n, nn, a, an, label) {
    const wanted = n.units(nn), observed = a.units(an);
    assert.equal(observed.length, wanted.length, label + ': complete semantic token inventory');
    for (let index = 0; index < wanted.length; index++) {
      const x = wanted[index], y = observed[index]; assert.equal(y.text, x.text, label + ': token spelling');
      assert.equal(!!y.reference, !!x.reference, label + ': same generated-reference role');
      if (x.reference) assert.deepEqual(y.owner, x.owner, label + ': generated-reference interior');
      else assert.deepEqual(y.owners, x.owners, label + ': complete source UTF-16 interiors');
      for (const [view, token] of [[native, x], [actual, y]]) { view.cover(token.start, token.end); view.roleStarts.set(token.start, native.owner(x.start)); view.roleEnds.set(token.end, native.owner(x.end)); }
      actual.nativeStarts.set(y.start, x.start); actual.nativeEnds.set(y.end, x.end);
      if (index) compareGap(native, wanted[index - 1].end, x.start, actual, observed[index - 1].end, y.start, label + ': ordered physical gap');
    }
  }
  function prepare({n, a, contract, original}) {
    phases = calls.compare(ts, native, actual, [originalText]);
    phases.functions = [];
    phases.publications = [];
    // Close every already-proved decorator statement at its own Native logical phase.
    for(const pair of phases.statementPairs){
      assert(covered(native,pair.native)&&covered(actual,pair.actual),'complete decorated statement character proof before phase accounting');
      assert.deepEqual(actual.owner(pair.actual.end),native.owner(pair.native.end),'exact complete decorator statement logical endpoint');
      for(const [view,node]of [[native,pair.native],[actual,pair.actual]]){
        view.roleStarts.set(node.getStart(view.source),native.owner(pair.native.getStart(native.source)));
        view.roleEnds.set(node.end,native.owner(pair.native.end));
      }
      actual.nativeStarts.set(pair.actual.getStart(actual.source),pair.native.getStart(native.source));
      actual.nativeEnds.set(pair.actual.end,pair.native.end);
    }
    const sourceClasses = original.statements.filter(ts.isClassDeclaration), classNames = new Set(sourceClasses.map(node => node.name.text));
    for (const pair of phases.pairs) {
      if (pair.kind !== 'decorate') continue;
      const nn = pair.native.parent, an = pair.actual.parent;
      if (!ts.isBinaryExpression(nn) || !ts.isBinaryExpression(an)) continue;
      for (const node of [nn, an]) assert(node.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(node.left) && classNames.has(node.left.text), 'source-bound class decoration assignment');
      pairUnits(phases.native, nn, phases.actual, an, 'complete class-decoration assignment');
      const wrapper = nn.parent;
      if (ts.isBinaryExpression(wrapper) && wrapper.right === nn) {
        assert(wrapper.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isPropertyAccessExpression(wrapper.left)
          && wrapper.left.expression.getText(native.source) === 'exports' && wrapper.left.name.text === nn.left.text,
        'Native combined final class publication');
        assert(ts.isExpressionStatement(wrapper.parent) && ts.isExpressionStatement(an.parent));
        const ns = wrapper.parent, as = an.parent, position = a.sourceNodes.indexOf(as), publication = a.sourceNodes[position + 1];
        assert(position >= 0 && publication && ts.isExpressionStatement(publication) && ts.isBinaryExpression(publication.expression));
        const update = publication.expression;
        assert(update.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isPropertyAccessExpression(update.left)
          && update.left.expression.getText(actual.source) === 'exports' && update.left.name.text === an.left.text
          && ts.isIdentifier(update.right) && update.right.text === an.left.text, 'immediate final class replacement publication');
        native.noSource(ns.getStart(native.source), nn.getStart(native.source), 'generated combined final class publication prefix');
        actual.noSource(publication.getStart(actual.source), publication.end, 'generated final class replacement publication');
        phases.publications.push(publication);
        const nt = native.tokens(ns).at(-1), at = actual.tokens(as).at(-1);
        assert.equal(nt.text, ';'); assert.equal(at.text, ';'); contract.pair([nt], [at], 'class-decoration statement completion');
      }
    }
    for (const fn of original.statements.filter(ts.isFunctionDeclaration)) {
      assert(fn.parameters.every(p => ts.isIdentifier(p.name) && !p.dotDotDotToken && !p.questionToken && !p.initializer), 'complete original fixed-arity decorator callable inventory');
      const nf = n.sourceNodes.filter(node => ts.isFunctionDeclaration(node) && node.name?.text === fn.name.text);
      const af = a.sourceNodes.filter(node => ts.isFunctionDeclaration(node) && node.name?.text === fn.name.text);
      assert.equal(nf.length, 1); assert.equal(af.length, 1);
      for(const emitted of [nf[0],af[0]]) assert.deepEqual(emitted.parameters.map(p => p.name.getText(emitted.getSourceFile())), fn.parameters.map(p => p.name.getText(original)), 'source-derived complete fixed signature');
      phases.functions.push(rest.compareFunction(ts, native, nf[0], actual, af[0], fn, options));
      native.roleStarts.set(nf[0].getStart(native.source), native.owner(nf[0].getStart(native.source)));
      actual.roleStarts.set(af[0].getStart(actual.source), native.owner(nf[0].getStart(native.source)));
      native.roleEnds.set(nf[0].end, native.owner(nf[0].end)); actual.roleEnds.set(af[0].end, native.owner(nf[0].end));
      actual.nativeStarts.set(af[0].getStart(actual.source), nf[0].getStart(native.source)); actual.nativeEnds.set(af[0].end, nf[0].end);
    }
    const sourceNames = phases.native.sourceNames;
    const expectedKinds = [...new Set(phases.native.units(native.source).filter(token => token.text.startsWith('$helper.')).map(token => token.text.slice('$helper.'.length)))].sort();
    function imports(item, state, blue) {
      const namespaces = state.tslib, bindings = new Set(state.bindings.keys());
      for (const node of item.sourceNodes) {
        if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier) && node.moduleSpecifier.text === 'tslib') {
          assert(options.importHelpers && node.importClause && !node.importClause.isTypeOnly);
          const named = node.importClause.namedBindings;
          if (blue) { assert(ts.isNamespaceImport(named) && namespaces.has(named.name.text)); item.exact(node, `import * as ${named.name.text} from 'tslib';`); }
          else { assert(ts.isNamedImports(named)); assert.deepEqual(named.elements.map(element => helper.helpers.get(element.name.text)).sort(), expectedKinds); assert(named.elements.every(element => !element.propertyName && !element.isTypeOnly)); }
          item.view.noSource(node.getStart(item.view.source), node.end, 'complete generated decorator import');
        }
        if (ts.isVariableStatement(node) && node.declarationList.declarations.length === 1) {
          const declaration = node.declarationList.declarations[0];
          if (ts.isIdentifier(declaration.name) && namespaces.has(declaration.name.text) && declaration.initializer) {
            assert(options.importHelpers && ts.isCallExpression(declaration.initializer) && declaration.initializer.expression.getText(item.view.source) === 'require');
            assert.equal(declaration.initializer.arguments.length, 1); assert(ts.isStringLiteral(declaration.initializer.arguments[0]) && declaration.initializer.arguments[0].text === 'tslib');
            assert(!sourceNames.has(declaration.name.text)); item.view.noSource(node.getStart(item.view.source), node.end, 'complete generated decorator namespace acquisition');
          }
        }
      }
      if (item.format !== 'system') return;
      const originalVariables = original.statements.filter(ts.isVariableStatement);
      const locals = originalVariables.filter(node => !blue || !node.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.ExportKeyword))
        .flatMap(node => node.declarationList.declarations.map(declaration => { assert(ts.isIdentifier(declaration.name)); return declaration.name.text; }));
      const expected = new Set([...classNames, ...locals, ...namespaces, ...(blue ? bindings : [])]);
      const seen = [];
      for (const node of item.sourceNodes) {
        if (!ts.isVariableStatement(node) || !node.declarationList.declarations.every(declaration => !declaration.initializer)) continue;
        const names = node.declarationList.declarations.map(declaration => { assert(ts.isIdentifier(declaration.name)); return declaration.name.text; });
        assert(names.every(name => expected.has(name)), 'hoisted binding must belong to original declarations or proved helpers');
        seen.push(...names); item.view.noSource(node.getStart(item.view.source), node.end, 'complete class/decorator/source System hoisted binding inventory');
      }
      assert.deepEqual(seen.sort(), [...expected].sort(), 'exact complete System source/decorator hoisting');
      item.setterNodes.forEach((setter, index) => {
        assert.equal(item.dependencies[index], 'tslib'); assert.equal(namespaces.size, 1); const binding = [...namespaces][0];
        assert(ts.isFunctionExpression(setter) && setter.parameters.length === 1 && ts.isIdentifier(setter.parameters[0].name));
        const parameter = setter.parameters[0].name.text; assert(!sourceNames.has(binding));
        item.exact(setter, blue ? `function(${parameter}){${item.system.stem}_imports['tslib']=${parameter};${binding}=${parameter};${item.system.stem}_refresh();}` : `function(${parameter}){${binding}=${parameter};}`);
        item.view.noSource(setter.getStart(item.view.source), setter.end, 'complete decorator provider System setter ABI');
      });
      for (const node of item.sourceNodes) {
        if (!ts.isExpressionStatement(node) || !ts.isCallExpression(node.expression)) continue;
        const call = node.expression;
        if (call.expression.getText(item.view.source) !== item.system.publisher || call.arguments.length !== 2) continue;
        const [name, value] = call.arguments;
        if (!ts.isStringLiteral(name) || !classNames.has(name.text) || !ts.isBinaryExpression(value)) continue;
        assert(value.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(value.left) && value.left.text === name.text);
        assert(covered(item.view, value), 'decorator assignment has already proved every UTF-16 role');
        item.view.noSource(node.getStart(item.view.source), value.getStart(item.view.source), 'generated System class replacement publication prefix');
        const sourceClass = sourceClasses.find(source => source.name.text === name.text);
        const ending = original.getLineAndCharacterOfPosition(sourceClass.end), owner = [0, ending.line, ending.character];
        assert.deepEqual(item.view.tokens(node).filter(token => token.start >= value.end).map(token => token.text), [')', ';'], 'complete Native System class publication completion shape');
        for (let offset = value.end; offset < node.end; offset++) assert.deepEqual(item.view.owner(offset), owner, 'Native System publication retains exact logical class completion');
        item.view.cover(value.end, node.end); item.view.roleStarts.set(value.end, owner); item.view.roleEnds.set(node.end, owner);
        const paired = phases.pairs.find(pair => pair.native.parent === value); assert(paired && ts.isExpressionStatement(paired.actual.parent.parent));
        const statement = paired.actual.parent.parent, token = actual.tokens(statement).at(-1); assert.equal(token.text, ';');
        assert(token.owners.every(value => JSON.stringify(value) === JSON.stringify(owner)), 'actual decorator statement retains the Native logical class completion');
        actual.cover(token.start, token.end); actual.roleStarts.set(token.start, owner); actual.roleEnds.set(token.end, owner);
        actual.nativeStarts.set(token.start, item.view.tokens(node).at(-1).start); actual.nativeEnds.set(token.end, node.end);
      }
    }
    imports(n, phases.native, false); imports(a, phases.actual, true);
    return phases;
  }
  return moduleContract.compare(ts, native, actual, options, originalText, {
    dependencies: options.importHelpers ? ['tslib'] : [], prepare,
    classNode(item, name) {
      const unwrap = node => { while (ts.isParenthesizedExpression(node)) node = node.expression; return node; };
      function construction(node) {
        const value = unwrap(node);
        return ts.isClassExpression(value) && value.name?.text === name
          || ts.isCallExpression(value) && ts.isFunctionExpression(unwrap(value.expression));
      }
      const candidates = item.sourceNodes.filter(node => ts.isClassDeclaration(node) && node.name?.text === name
        || ts.isVariableStatement(node) && node.declarationList.declarations.length === 1
          && node.declarationList.declarations[0].name.getText(item.view.source) === name
          && node.declarationList.declarations[0].initializer && construction(node.declarationList.declarations[0].initializer)
        || ts.isExpressionStatement(node) && ts.isBinaryExpression(node.expression)
          && node.expression.operatorToken.kind === ts.SyntaxKind.EqualsToken
          && ts.isIdentifier(node.expression.left) && node.expression.left.text === name && construction(node.expression.right));
      assert.equal(candidates.length, 1, 'one complete source-bound class construction phase');
      return candidates[0];
    },
    classRoles(source, nn, an) { return {nativeExcluded: phases.statementPairs.map(pair => pair.native).filter(node => nn.pos <= node.pos && node.end <= nn.end),
      actualExcluded: phases.statementPairs.map(pair => pair.actual).filter(node => an.pos <= node.pos && node.end <= an.end)}; },
  });
}
module.exports = {compare};
