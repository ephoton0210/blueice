// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Read the two explicit generator ABIs into a bounded data-flow trace.
const assert = require('node:assert/strict');

class ChoiceNeeded extends Error {
  constructor(index) { super('continuation branch choice ' + index); this.index = index; }
}

function read(ts, view, node, dialect, originals, options = {}) {
  const identifiers = new Set(), bindings = new Set();
  for (const source of originals) {
    const original = ts.createSourceFile('original.ts', source, ts.ScriptTarget.Latest, true);
    function binding(name) {
      if (ts.isIdentifier(name)) bindings.add(name.text);
      else for (const element of name.elements) if (ts.isBindingElement(element)) binding(element.name);
    }
    function visit(node) {
      if (ts.isIdentifier(node)) identifiers.add(node.text);
      if (ts.isVariableDeclaration(node) || ts.isParameter(node)) binding(node.name);
      ts.forEachChild(node, visit);
    }
    visit(original);
  }
  if (options.generatorReference?.abi === 'tslib-adapter') {
    assert.equal(dialect, 'blue');
    return require('./ge5_tslib_generator_adapter.cjs').read(ts, view, node, originals, options, identifiers, bindings);
  }
  const body = options.bodyStatements ?? node.body.statements, returned = body.at(-1);
  assert(ts.isReturnStatement(returned) && ts.isCallExpression(returned.expression));
  const call = returned.expression, blue = dialect === 'blue';
  if (options.generatorReference) {assert(!blue&&options.generatorReference.abi==='native');require('./ge5_tslib_generator_adapter.cjs').reference(ts,call.expression,options.generatorReference,identifiers);}
  else assert.match(call.expression.getText(), blue ? /^__blueice_target_generator_\d+$/ : /^__generator(?:_\d+)?$/);
  assert.equal(call.arguments.length, blue ? 3 : 2); assert.equal(call.arguments[0].kind, ts.SyntaxKind.ThisKeyword);
  const callback = call.arguments.at(-1); assert(ts.isFunctionExpression(callback) && !callback.asteriskToken && !callback.modifiers?.length);
  assert.equal(callback.parameters.length, 1);
  const context = callback.parameters[0].name.getText();
  assert(!identifiers.has(context), 'fresh continuation context');
  const slots = new Set(), userSlots = new Set();
  for (const statement of body.slice(0, -1)) {
    assert(ts.isVariableStatement(statement));
    for (const declaration of statement.declarationList.declarations) {
      assert(ts.isIdentifier(declaration.name)); const name = declaration.name.text;
      assert(bindings.has(name) || !identifiers.has(name), 'fresh generated slot or original user binding');
      if (declaration.initializer) {
        assert(blue && /^__blueice_target_generator_arguments_\d+$/.test(name));
        assert.equal(declaration.initializer.getText(), 'arguments');
      } else { assert(!slots.has(name)); slots.add(name); if (bindings.has(name)) userSlots.add(name); }
    }
  }
  assert.equal(callback.body.statements.length, 1);
  let dispatcher = callback.body.statements[0];
  if (blue) {
    assert(ts.isWhileStatement(dispatcher) && dispatcher.expression.kind === ts.SyntaxKind.TrueKeyword);
    assert(ts.isBlock(dispatcher.statement) && dispatcher.statement.statements.length === 1);
    dispatcher = dispatcher.statement.statements[0];
  }
  assert(ts.isSwitchStatement(dispatcher)); assert.equal(dispatcher.expression.getText(), context + '.label');
  const states = new Map(); let defaults = 0;
  for (const clause of dispatcher.caseBlock.clauses) {
    if (ts.isDefaultClause(clause)) {
      assert(blue); defaults++;
      assert.equal(clause.statements.map(node => node.getText()).join('').replace(/\s/g, ''), 'thrownewError("Invalidgeneratorcontinuation");'); continue;
    }
    assert(ts.isCaseClause(clause) && ts.isNumericLiteral(clause.expression));
    const number = Number(clause.expression.text); assert(Number.isSafeInteger(number) && number >= 0 && !states.has(number));
    states.set(number, [...clause.statements]);
  }
  assert.equal(defaults, blue ? 1 : 0); assert(states.size > 0 && states.size <= 512);
  const entry = blue ? Number(call.arguments[1].getText()) : 0; assert(Number.isSafeInteger(entry) && states.has(entry));
  const result = { view, node, returned, call, callback, dispatcher, context, slots, userSlots, states, entry, blue,
    identifiers, originals, computed: Boolean(options.computed), switch: Boolean(options.switch),
    exceptions: options.exceptions === true ? 'finally-return' : options.exceptions || null, loop: Boolean(options.loop),
    delegation: Boolean(options.delegation), helper: call.expression.getText(), valuesReference: options.valuesReference };
  if (options.lexicalReceiver) result.lexicalReceiver = require('./ge5_lexical_arrow_receiver.cjs').bind(ts, result, options.lexicalReceiver);
  if (options.superMember) result.superMember = require('./ge5_async_super_member_roles.cjs').bind(result, options.superMember);
  if (result.loop) require('./ge5_loop_state.cjs').bind(ts, result);
  return result;
}

function trace(ts, machine, choices = []) {
  const { view, context, slots, userSlots, states, blue } = machine;
  const switchProfile = machine.exceptions === 'switch-delegate-finally'
    ? require('./ge5_generator_switch_machine.cjs') : null;
  if (switchProfile) switchProfile.checked(machine);
  const environment = new Map(), definitions = [], values = [], events = [], controls = [], visited = new Set(), generatedGroups = [], reads = [];
  const regions = machine.loop ? require('./ge5_loop_regions.cjs').regions(ts, machine)
    : machine.exceptions ? require('./ge5_exception_regions.cjs').regions(ts, machine) : null;
  const loop = machine.loop ? require('./ge5_loop_state.cjs').state(machine) : null;
  let state = machine.entry, pending = null, resumed = 0, decisions = 0, steps = 0;
  const suspensions = () => events.filter(event => event.kind === 'yield' || event.kind === 'delegate').length;
  function value(kind, node, fields) {
    const entry = machine.superMember ? require('./ge5_async_super_member_roles.cjs').entry(machine.superMember, node) : undefined;
    const result = { kind, node, entry: entry ?? view.owner(node.getStart(view.source)), uses: [], initializerUses: [], returnUses: [], referenceUses: [], phase: regions?.phase(), ...fields };
    values.push(result); return result;
  }
  function finished(completion = null) {
    assert.equal(pending, null);
    assert.equal(resumed, suspensions());
    assert.equal(decisions, choices.length, 'complete branch choice sequence');
    return { machine, events, definitions, values, controls, visited, choices, generatedGroups, reads,
      regions: regions?.registrations, regionRouter: regions, completion };
  }
  function expression(node, usage = {}) {
    if (machine.superMember?.read === node) {
      const result = value('super-member', node, {text: machine.superMember.role, entry: machine.superMember.entry, capability: machine.superMember});
      reads.push({value: result, yielded: suspensions(), resumed}); return result;
    }
    if (machine.lexicalReceiver?.read === node) return value('receiver', node, { text: machine.lexicalReceiver.role });
    if (node.kind === ts.SyntaxKind.ThisKeyword) return value('this', node, {});
    if (ts.isNumericLiteral(node) || ts.isStringLiteral(node)
      || [ts.SyntaxKind.TrueKeyword, ts.SyntaxKind.FalseKeyword, ts.SyntaxKind.NullKeyword].includes(node.kind)) return value('literal', node, { text: node.getText() });
    if (ts.isIdentifier(node)) {
      if (userSlots.has(node.text) || !slots.has(node.text) && machine.identifiers.has(node.text)) {
        const text = machine.loopBinding?.alias === node.text ? machine.loopBinding.name : node.text;
        const result=value('user',node,{text,known:loop?.snapshot(node.text)});reads.push({value:result,yielded:events.filter(event=>event.kind==='yield').length,resumed});return result;
      }
      assert(slots.has(node.text) && environment.has(node.text), 'read an initialized local slot: ' + node.text);
      const result = environment.get(node.text);
      if (result.kind === 'exception') return value('catch-read', node, { payload: result });
      if(usage.reference)result.referenceUses.push({node,reference:usage.reference});
      else if (usage.initializer) result.initializerUses.push({ node, binding: usage.initializer });
      else if (usage.returned) result.returnUses.push({ node, statement: usage.returned });
      else if (!usage.condition && !usage.transport) result.uses.push(node);
      return result;
    }
    if (ts.isParenthesizedExpression(node)) {
      if (machine.lexicalReceiver?.conciseResultGroup === node) {
        const inner = expression(node.expression, usage);
        generatedGroups.push({ node, value: inner, concise: true }); return inner;
      }
      const open = view.owner(node.getStart(view.source)), close = view.owner(node.end - 1);
      if (open === null && close === null) {
        view.noSource(node.getStart(view.source), node.getStart(view.source) + 1, 'additional arithmetic grouping');
        view.noSource(node.end - 1, node.end, 'additional arithmetic grouping');
        return expression(node.expression, usage);
      }
      const contents = view.decoded.metadata.sourcesContent?.[open?.[0]]
        ?? (view.decoded.metadata.sources.length === 1 && machine.originals.length === 1 ? machine.originals[0] : null);
      const original = open && contents?.split('\n')[open[1]]?.[open[2]];
      if (blue && (open === null || original === '`')) {
        const inner = expression(node.expression, usage);
        if (original === '`') inner.entry = open;
        generatedGroups.push({ node, value: inner, template: original === '`' }); return inner;
      }
      if (!blue && original !== '(') {
        const inner = expression(node.expression, usage); generatedGroups.push({ node, value: inner }); return inner;
      }
      return value('group', node, { inner: expression(node.expression) });
    }
    if (ts.isBinaryExpression(node) && !ts.isAssignmentOperator(node.operatorToken.kind)) {
      assert(['+', '-', '*', '/', '%', '**'].includes(node.operatorToken.getText())
        || machine.loop && ['<', '==='].includes(node.operatorToken.getText()), 'reviewed arithmetic operation');
      return value('binary', node, { operator: node.operatorToken.getText(), left: expression(node.left), right: expression(node.right) });
    }
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken) {
      assert(!blue && ts.isIdentifier(node.left) && slots.has(node.left.text) && !userSlots.has(node.left.text));
      const result = expression(node.right); environment.set(node.left.text, result);
      definitions.push({ statement: node, left: node.left, right: node.right, value: result }); return result;
    }
    if (ts.isCallExpression(node)) {
      if (node.expression.getText() === context + '.sent') {
        assert.equal(node.arguments.length, 0);
        if (pending === null && machine.exceptions === 'catch-finally') return value('exception', node, { thrown: regions.exception() });
        assert(pending !== null, 'sent follows a yield'); const id = pending; pending = null; resumed++;
        return value('sent', node, { id });
      }
      const intrinsic = blue && node.expression.getText() === 'Function.prototype.apply.call';
      const nativeApply = !blue && ts.isPropertyAccessExpression(node.expression) && node.expression.name.text === 'apply'
        && ts.isIdentifier(node.expression.expression) && slots.has(node.expression.expression.text) && !userSlots.has(node.expression.expression.text);
      if (intrinsic || nativeApply) {
        assert.equal(node.arguments.length, intrinsic ? 3 : 2);
        const callee = expression(intrinsic ? node.arguments[0] : node.expression.expression, { transport: true });
        assert.equal(callee.kind, 'property');
        const receiverNode = node.arguments[intrinsic ? 1 : 0];
        const receiver = !blue && ts.isStringLiteral(receiverNode) && callee.object.kind === 'literal'
          && receiverNode.getText() === callee.object.text ? callee.object : expression(receiverNode, { transport: true });
        assert(receiver === callee.object, 'invoke the captured method on its original receiver');
        const array = node.arguments.at(-1); assert(ts.isArrayLiteralExpression(array));
        const arguments_ = array.elements.map(argument => expression(argument, { transport: true }));
        return value('call', node, { entry: blue ? callee.entry : view.owner(node.getStart(view.source)), callee, arguments: arguments_, dispatch: { array, intrinsic } });
      }
      return value('call', node, { callee: expression(node.expression), arguments: node.arguments.map(argument => expression(argument)) });
    }
    if(ts.isArrayLiteralExpression(node)) {
      assert(node.elements.every(element=>!ts.isOmittedExpression(element)&&!ts.isSpreadElement(element)),'reviewed dense array literal');
      return value('array',node,{elements:node.elements.map(element=>expression(element))});
    }
    if(ts.isElementAccessExpression(node)) {
      assert(node.argumentExpression);const reference=usage.reference?{reference:node}:{};return value('element',node,{object:expression(node.expression,reference),key:expression(node.argumentExpression,reference),reference:Boolean(usage.reference)});
    }
    if (ts.isPropertyAccessExpression(node)) {
      if (machine.blue && machine.exceptions === 'catch-finally' && node.getText() === context + '.exception') {
        return value('exception', node, { thrown: regions.exception() });
      }
      return value('property', node, { object: expression(node.expression), name: node.name.getText() });
    }
    assert.fail('unreviewed continuation expression: ' + node.getText());
  }
  function object(node, name) {
    assert(ts.isObjectLiteralExpression(node) && node.properties.length === 2);
    const found = node.properties.find(property => ts.isPropertyAssignment(property) && property.name.getText() === name);
    assert(found); return found.initializer;
  }
  function label(statement) {
    if (!ts.isExpressionStatement(statement) || !ts.isBinaryExpression(statement.expression)) return null;
    const { left, right, operatorToken } = statement.expression;
    if (left.getText() !== context + '.label') return null;
    assert.equal(operatorToken.kind, ts.SyntaxKind.EqualsToken);
    if (ts.isNumericLiteral(right)) return { target: Number(right.text) };
    assert(blue && ts.isConditionalExpression(right));
    assert(ts.isNumericLiteral(right.whenTrue) && ts.isNumericLiteral(right.whenFalse));
    return { test: right.condition, yes: Number(right.whenTrue.text), no: Number(right.whenFalse.text) };
  }
  function condition(node) {
    let negative = false, current = node;
    while (ts.isParenthesizedExpression(current) || ts.isPrefixUnaryExpression(current)) {
      if (ts.isParenthesizedExpression(current)) {
        const owner = view.owner(current.getStart(view.source));
        const source = view.decoded.metadata.sourcesContent?.[owner?.[0]]
          ?? (view.decoded.metadata.sources.length === 1 && machine.originals.length === 1 ? machine.originals[0] : null);
        if (owner && source?.split('\n')[owner[1]]?.[owner[2]] === '(') break;
        current = current.expression;
      }
      else { assert.equal(current.operator, ts.SyntaxKind.ExclamationToken); negative = !negative; current = current.operand; }
    }
    return { value: expression(current, { condition: true }), negative };
  }
  function choose(test, fields) {
    if (!loop && choices.length <= decisions) throw new ChoiceNeeded(decisions);
    const chosen = loop ? loop.condition(test.value) : choices[decisions++]; assert.equal(typeof chosen, 'boolean');
    events.push({ kind: 'branch', value: test.value, chosen, ...fields });
    return chosen !== test.negative;
  }
  while (true) {
    assert(++steps <= 2048, 'bounded continuation visits');
    assert(states.has(state) && (machine.loop || !visited.has(state)), 'finite reachable continuation'); visited.add(state);
    const statements = states.get(state); let next = blue ? null : state + 1, stopped = false;
    for (let index = 0; index < statements.length; index++) {
      const statement = statements[index], target = label(statement);
      const entered = regions?.enter(statement, state);
      if (entered) {
        controls.push(statement);
        if (blue) {
          assert.equal(index, statements.length - 1, 'region entry is the complete state terminator');
          next = entered.next; stopped = true; break;
        }
        continue;
      }
      const generated = regions?.generated(statement);
      if (generated) {
        assert.equal(index, statements.length - 1, 'region transfer is the complete state terminator');
        controls.push(statement);
        if (generated.done) return finished(generated.value);
        next = generated.next; stopped = true; break;
      }
      if (ts.isSwitchStatement(statement)) {
        assert(machine.switch, 'reviewed source switch dispatch');
        const selector = expression(statement.expression, { transport: true });
        let matched = false;
        for (const clause of statement.caseBlock.clauses) {
          if (ts.isDefaultClause(clause)) {
            assert(blue && clause === statement.caseBlock.clauses.at(-1));
            if (!matched) {
              assert.equal(clause.statements.length, 2);
              const jump = label(clause.statements[0]); assert(jump && !jump.test);
              assert(ts.isContinueStatement(clause.statements[1]) && !clause.statements[1].label);
              next = jump.target; stopped = true;
            }
            controls.push(clause); continue;
          }
          assert(ts.isCaseClause(clause));
          if (matched) continue;
          const test = expression(clause.expression);
          let opcode, destination;
          if (blue) {
            assert.equal(clause.statements.length, 2);
            opcode = clause.statements[0]; const jump = label(opcode); assert(jump && !jump.test);
            assert(ts.isContinueStatement(clause.statements[1]) && !clause.statements[1].label);
            controls.push(clause.statements[1]); destination = jump.target;
          } else {
            assert.equal(clause.statements.length, 1); opcode = clause.statements[0];
            assert(ts.isReturnStatement(opcode) && ts.isArrayLiteralExpression(opcode.expression));
            const elements = opcode.expression.elements; assert.equal(elements.length, 2);
            assert.equal(elements[0].getText(), '3'); assert(ts.isNumericLiteral(elements[1]));
            destination = Number(elements[1].text);
          }
          assert(states.has(destination));
          if (choices.length <= decisions) throw new ChoiceNeeded(decisions);
          const chosen = choices[decisions++]; assert.equal(typeof chosen, 'boolean');
          events.push({ kind: 'case', statement, clause, opcode, value: selector, test, chosen });
          if (chosen) { next = destination; matched = true; stopped = true; }
        }
        if (matched || stopped) break;
        continue;
      }
      if (target !== null) {
        if (target.test) {
          const selected = switchProfile?.selection(machine, target.test);
          if (selected) {
            if (choices.length <= decisions) throw new ChoiceNeeded(decisions);
            const chosen = choices[decisions++]; assert.equal(typeof chosen, 'boolean');
            events.push({kind: 'case', statement, opcode: statement, clause: null,
              value: expression(target.test.left, {transport: true}), test: expression(target.test.right, {transport: true}),
              chosen, projection: selected});
            assert(states.has(target.yes) && states.has(target.no)); next = chosen ? target.yes : target.no;
            continue;
          }
          const test = condition(target.test), yes = choose(test, { statement });
          assert(states.has(target.yes) && states.has(target.no)); next = yes ? target.yes : target.no;
        } else { assert(states.has(target.target)); next = target.target; }
        if (!machine.loop || !target.test) controls.push(statement); continue;
      }
      if (ts.isIfStatement(statement)) {
        assert(!blue && !statement.elseStatement && ts.isReturnStatement(statement.thenStatement));
        const opcode = statement.thenStatement, array = opcode.expression;
        assert(ts.isArrayLiteralExpression(array) && array.elements.length === 2 && array.elements[0].getText() === '3' && ts.isNumericLiteral(array.elements[1]));
        const test = condition(statement.expression), yes = choose(test, { statement, opcode });
        if (yes) {
          next = Number(array.elements[1].text); assert(states.has(next));
          if (machine.loop && view.owner(opcode.getStart(view.source))) {
            events.push({ kind: 'jump', statement: opcode, target: next }); next = regions.jump(next, 0).next;
          }
          stopped = true; break;
        }
        continue;
      }
      if (ts.isContinueStatement(statement)) {
        assert(blue && next !== null && !statement.label && index === statements.length - 1);
        controls.push(statement); stopped = true; break;
      }
      if (ts.isExpressionStatement(statement)) {
        if (ts.isCallExpression(statement.expression) && statement.expression.expression.getText() === context + '.sent') {
          events.push({ kind: 'resume', statement, value: expression(statement.expression) }); continue;
        }
        if (['catch-finally', 'delegate-finally', 'normal-finally', 'pending-return-finally'].includes(machine.exceptions) && ts.isCallExpression(statement.expression)) {
          events.push({ kind: 'effect', statement, value: expression(statement.expression) }); continue;
        }
        if (switchProfile && ts.isBinaryExpression(statement.expression)
          && statement.expression.operatorToken.kind === ts.SyntaxKind.PlusEqualsToken) {
          assert.equal(regions.phase(), 'finally');
          const update = statement.expression;
          assert(ts.isIdentifier(update.left) && update.left.text === switchProfile.checked(machine).origin.parameter);
          events.push({kind: 'effect', statement, value: value('compound-assignment', update,
            {left: expression(update.left), right: expression(update.right), operator: '+='})});
          continue;
        }
        if (machine.loop && (ts.isPostfixUnaryExpression(statement.expression) || ts.isPrefixUnaryExpression(statement.expression))) {
          const update = statement.expression; assert(ts.isIdentifier(update.operand));
          const operand = expression(update.operand), operator = ts.tokenToString(update.operator);
          const change = loop.update(update.operand.text, operator);
          events.push({ kind: 'update', statement, value: operand, operator, postfix: ts.isPostfixUnaryExpression(update), ...change }); continue;
        }
        let assignment = statement.expression;
        if (ts.isParenthesizedExpression(assignment)) {
          assert(blue, 'reviewed generated assignment grouping');
          view.noSource(assignment.getStart(view.source), assignment.getStart(view.source) + 1, 'generated assignment grouping');
          view.noSource(assignment.end - 1, assignment.end, 'generated assignment grouping');
          assignment = assignment.expression;
        }
        assert(ts.isBinaryExpression(assignment) && (assignment.operatorToken.kind === ts.SyntaxKind.EqualsToken
          || machine.switch && assignment.operatorToken.kind === ts.SyntaxKind.PlusEqualsToken));
        const { left, right } = assignment;
        if(ts.isElementAccessExpression(left)) {
          assert(machine.computed,'reviewed computed write profile');
          const reference=expression(left,{reference:left}),result=expression(right);
          events.push({kind:'write',statement,assignment,left,right,reference,value:result});continue;
        }
        assert(ts.isIdentifier(left) && slots.has(left.text));
        const user = userSlots.has(left.text), result = expression(right, user ? { initializer: left } : {});
        environment.set(left.text, result);
        if (user && loop) loop.store(left.text, result);
        if (user) events.push({ kind: 'store', statement, assignment, left, right, value: result });
        else definitions.push({ statement, left, right, value: result });
        continue;
      }
      if (machine.exceptions === 'catch-finally' && ts.isThrowStatement(statement)) {
        assert.equal(index, statements.length - 1, 'source throw is the complete state terminator');
        const result = expression(statement.expression);
        events.push({ kind: 'throw', statement, operand: statement.expression, root: statement.expression, value: result });
        const completion = regions.complete('throw', result);
        assert(!completion.done); next = completion.next; stopped = true; break;
      }
      assert(ts.isReturnStatement(statement) && index === statements.length - 1);
      let kind, operand, root;
      if (blue) {
        if ((machine.switch || machine.loop) && ts.isObjectLiteralExpression(statement.expression)
          && statement.expression.properties.some(property => ts.isPropertyAssignment(property)
            && property.name.getText() === 'kind' && property.initializer.getText() === '"jump"')) {
          const properties = statement.expression.properties;
          assert.deepEqual(properties.map(property => property.name.getText()), ['kind', 'label', 'depth']);
          assert(properties.every(ts.isPropertyAssignment));
          assert.equal(properties[2].initializer.getText(), switchProfile ? '1' : '0', 'switch break retains exactly its enclosing region depth');
          if (switchProfile) assert.equal(regions.phase(), 'body');
          assert(ts.isNumericLiteral(properties[1].initializer));
          next = Number(properties[1].initializer.text); assert(states.has(next));
          events.push({ kind: 'jump', statement, target: next });
          if (machine.loop) next = regions.jump(next, 0).next;
          stopped = true; break;
        }
        kind = object(statement.expression, 'kind').text; root = object(statement.expression, 'value');
        assert(ts.isParenthesizedExpression(root), 'completion protocol grouping');
        operand = root.expression;
      } else {
        assert(ts.isArrayLiteralExpression(statement.expression));
        const elements = statement.expression.elements; assert(elements.length === 2 && ts.isNumericLiteral(elements[0]));
        if (elements[0].text === '3') {
          assert(ts.isNumericLiteral(elements[1])); next = Number(elements[1].text); assert(states.has(next));
          if (machine.switch && view.owner(statement.getStart(view.source))) events.push({ kind: 'jump', statement });
          else controls.push(statement);
          stopped = true; break;
        }
        kind = ({ 4: 'yield', 2: 'return', ...(machine.delegation ? { 5: 'delegate' } : {}) })[elements[0].text]; operand = elements[1]; root = operand;
      }
      assert(['yield', 'return'].includes(kind) || machine.delegation && kind === 'delegate'
        || machine.exceptions === 'catch-finally' && kind === 'throw');
      let delegateWrapper = null;
      if (kind === 'delegate' && !blue) {
        delegateWrapper = operand;
        assert(ts.isCallExpression(delegateWrapper));
        if (switchProfile) require('./ge5_tslib_generator_adapter.cjs').reference(ts, delegateWrapper.expression,
          machine.valuesReference, machine.identifiers, '__values');
        else {assert(/^__values(?:_\d+)?$/.test(delegateWrapper.expression.getText()));assert(!machine.identifiers.has(delegateWrapper.expression.getText()));}
        assert.equal(delegateWrapper.arguments.length, 1); operand = delegateWrapper.arguments[0];
        if (switchProfile) switchProfile.denseDelegate(ts, machine, operand);
        else assert(ts.isCallExpression(operand), 'reviewed source delegate call');
      }
      let result;
      if (blue && (kind === 'yield' || kind === 'delegate')) {
        assert(ts.isIdentifier(operand) && slots.has(operand.text) && environment.has(operand.text));
        result = environment.get(operand.text);
        view.noSource(root.getStart(view.source), operand.end, 'yield transport slot/grouping');
      } else result = expression(operand, kind === 'return' ? { returned: statement } : {});
      events.push({ kind, statement, root, operand, value: result, delegateWrapper });
      if (kind === 'return' || kind === 'throw') {
        assert.equal(pending, null); assert.equal(resumed, suspensions());
        if (regions) {
          const completion = regions.complete(kind, result);
          if (!completion.done) { next = completion.next; stopped = true; break; }
        }
        assert.equal(decisions, choices.length, 'complete branch choice sequence');
        const unused = [...states].filter(([number]) => !visited.has(number));
        if (decisions === 0 && !regions) {
        assert(unused.length <= (blue ? 1 : 0), 'no unreviewed dead states');
        for (const [, statements] of unused) {
          assert.equal(statements.length, 1); const end = statements[0]; assert(ts.isReturnStatement(end));
          assert.equal(object(end.expression, 'kind').text, 'return');
          assert.equal(object(end.expression, 'value').getText().replace(/\s/g, ''), 'void0');
          view.noSource(end.getStart(view.source), end.end, 'unreachable implicit completion');
        }
        }
        return finished(result);
      }
      assert.equal(pending, null); pending = suspensions();
      assert(next !== null && states.has(next)); stopped = true; break;
    }
    assert(stopped || !blue, 'Blue continuation must transfer explicitly'); assert(next !== null && states.has(next)); state = next;
  }
}
function paths(ts, machine) {
  if (machine.asyncGeneratorAdapter) return require('./ge5_async_generator_machine.cjs').paths(ts, machine);
  const pending = [[]], result = [];
  while (pending.length) {
    assert(result.length + pending.length <= 128, 'bounded abstract branch paths'); const choices = pending.pop();
    try { result.push(trace(ts, machine, choices)); }
    catch (error) {
      if (!(error instanceof ChoiceNeeded)) throw error;
      assert.equal(error.index, choices.length); pending.push([...choices, false], [...choices, true]);
    }
  }
  const visited = new Set(result.flatMap(path => [...path.visited]));
  if (machine.exceptions) {
    result[0].regionRouter.audit(visited);
    return result;
  }
  const dead = [...machine.states].filter(([number]) => !visited.has(number));
  assert(dead.length <= (machine.blue ? 1 : 0), 'all paths cover every reviewed state');
  for (const [, statements] of dead) {
    assert.equal(statements.length, 1); const node = statements[0]; assert(ts.isReturnStatement(node));
    assert.equal(node.getText().replace(/\s/g, ''), 'return{kind:"return",value:void0};');
    machine.view.noSource(node.getStart(machine.view.source), node.end, 'unused implicit completion');
  }
  return result;
}
module.exports = { read, trace, paths };
