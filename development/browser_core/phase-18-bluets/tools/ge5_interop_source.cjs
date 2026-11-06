// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Import edges, original bindings and all source declarations precede transport.
const assert = require('node:assert/strict');
const path = require('node:path').posix;
const registry = new WeakMap();

function read(ts, files, entry) {
  const sources = Object.fromEntries(Object.entries(files).filter(([name]) =>
    !name.startsWith('node_modules/') && /\.[cm]?ts$/.test(name) && !/\.d\.[cm]?ts$/.test(name)));
  assert(Object.hasOwn(sources, entry), 'entry belongs to the original source graph');
  const identities = [], modules = {};
  function inventory(node) {
    const children = [];
    ts.forEachChild(node, child => { children.push(child); });
    identities.push({node, parent: node.parent, kind: node.kind, pos: node.pos, end: node.end,
      text: node.text, escapedText: node.escapedText, flags: node.flags, children});
    for (const child of children) inventory(child);
  }
  function identifier(node) { assert(ts.isIdentifier(node)); return node.text; }
  function resolve(filename, specifier) {
    assert(specifier.startsWith('.'), 'original dependency is a relative source edge');
    const resolved = path.normalize(path.join(path.dirname(filename), specifier)).replace(/\.[cm]?js$/, '.ts');
    assert(!resolved.startsWith('../') && !path.isAbsolute(resolved) && Object.hasOwn(sources, resolved),
      'original dependency escapes the supplied graph: ' + filename + ' -> ' + specifier);
    return resolved;
  }
  for (const [filename, text] of Object.entries(sources)) {
    const source = ts.createSourceFile(filename, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
    assert.equal(source.parseDiagnostics.length, 0, 'complete original source parses');
    const imports = [], declarations = [], exports = [];
    function exported(statement, local, node) {
      if (statement.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.ExportKeyword)) {
        const isDefault = statement.modifiers.some(modifier => modifier.kind === ts.SyntaxKind.DefaultKeyword);
        exports.push(Object.freeze({exported: isDefault ? 'default' : local, local, node}));
      }
    }
    for (const node of source.statements) {
      if (ts.isImportDeclaration(node)) {
        assert(!node.importClause?.isTypeOnly && ts.isStringLiteral(node.moduleSpecifier));
        const bindings = [], clause = node.importClause;
        if (clause?.name) bindings.push(Object.freeze({imported: 'default', local: identifier(clause.name), node: clause.name}));
        if (clause?.namedBindings) {
          const named = clause.namedBindings;
          if (ts.isNamespaceImport(named)) bindings.push(Object.freeze({imported: '*', local: identifier(named.name), node: named}));
          else {
            assert(ts.isNamedImports(named));
            for (const item of named.elements) {
              assert(!item.isTypeOnly);
              bindings.push(Object.freeze({imported: identifier(item.propertyName || item.name), local: identifier(item.name), node: item}));
            }
          }
        }
        imports.push(Object.freeze({node, specifier: node.moduleSpecifier.text,
          resolved: resolve(filename, node.moduleSpecifier.text), bindings: Object.freeze(bindings), star: false}));
      } else if (ts.isExportDeclaration(node)) {
        assert(!node.isTypeOnly && !node.exportClause && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier),
          'complete reviewed star export');
        imports.push(Object.freeze({node, specifier: node.moduleSpecifier.text,
          resolved: resolve(filename, node.moduleSpecifier.text), bindings: Object.freeze([]), star: true}));
      } else if (ts.isVariableStatement(node)) {
        for (const declaration of node.declarationList.declarations) {
          assert(declaration.initializer, 'original interop binding has its complete initializer');
          const name = identifier(declaration.name);
          declarations.push(Object.freeze({kind: 'variable', name, node: declaration, statement: node}));
          exported(node, name, declaration);
        }
      } else {
        assert(ts.isFunctionDeclaration(node) && node.name && node.body,
          'every original statement has an import, export or source declaration role');
        const name = identifier(node.name);
        declarations.push(Object.freeze({kind: 'function', name, node, statement: node,
          parameters: Object.freeze(node.parameters.map(parameter => identifier(parameter.name)))}));
        exported(node, name, node);
      }
    }
    inventory(source);
    modules[filename] = Object.freeze({filename, source, imports: Object.freeze(imports),
      declarations: Object.freeze(declarations), exports: Object.freeze(exports)});
  }
  const model = Object.freeze({entry, modules: Object.freeze(modules)}), capability = Object.freeze({});
  registry.set(capability, {ts, model, identities});
  return capability;
}

function checked(capability) {
  const record = registry.get(capability);
  assert(record, 'owned original import graph capability required');
  for (const item of record.identities) {
    const node = item.node, children = [];
    record.ts.forEachChild(node, child => { children.push(child); });
    for (const field of ['parent', 'kind', 'pos', 'end', 'text', 'escapedText', 'flags']) {
      assert.equal(node[field], item[field], 'original AST ' + field + ' remains unchanged');
    }
    assert.equal(children.length, item.children.length, 'complete original AST child inventory');
    for (let index = 0; index < children.length; index++) {
      assert.equal(children[index], item.children[index], 'exact original AST ancestry');
    }
  }
  return record.model;
}

function dependencies(capability, filename, format, blue) {
  const model = checked(capability), module = model.modules[filename];
  assert(module, 'module belongs to the exact original import graph');
  const edges = module.imports.map(edge => edge.specifier);
  return blue || String(format).toLowerCase() === 'system' ? [...new Set(edges)] : edges;
}

module.exports = {read, checked, dependencies};
