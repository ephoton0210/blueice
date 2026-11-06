// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Compare complete expression DAGs and every physical source grant in both ABIs.
const assert = require('node:assert/strict');
const { compareTokens, compareGap } = require('./ge5_role_stream.cjs');
const machine = require('./ge5_continuation_machine.cjs');

function compareFunction(ts, native, n, actual, a) {
  const nativePaths = machine.paths(ts, n), actualPaths = machine.paths(ts, a);
  assert.equal(actualPaths.length, nativePaths.length, 'complete abstract branch path inventory');
  const ranges = new Map([[native, []], [actual, []]]), paired = new Map(), predicates = new Map();
  function cover(view, start, end, trailing) {
    view.cover(start, end); ranges.get(view).push({ start, end, trailing });
  }
  function pairTokens(wanted, observed, label) {
    compareTokens(native, wanted, actual, observed, label);
    for (let index = 0; index < wanted.length; index++) {
      const x = wanted[index], y = observed[index], trailing = native.owner(x.end);
      cover(native, x.start, x.end, trailing); cover(actual, y.start, y.end, trailing);
    }
  }
  function uniform(view, start, end, owner, label, nullable = false) {
    for (let offset = start; offset < end; offset++) {
      const current = view.owner(offset);
      assert(nullable && current === null || JSON.stringify(current) === JSON.stringify(owner),
        label + ' at ' + offset + ': ' + JSON.stringify(current) + ' / ' + JSON.stringify(owner));
    }
    cover(view, start, end, owner);
  }
  function callTokens(view, value, context) {
    return view.tokens(value.node).map(token => {
      if (token.text !== context) return token;
      assert(token.owners.every(owner => JSON.stringify(owner) === JSON.stringify(token.owners[0])), 'uniform context alias');
      return { ...token, text: '$continuation', owners: [token.owners[0]] };
    });
  }
  function loopBinding(x, y) {
    assert(n.loop && a.loop); assert.equal(x.text, n.loopBinding.alias); assert.equal(y.text, a.loopBinding.alias);
    assert.equal(n.loopBinding.name, a.loopBinding.name);
    const entry = native.owner(x.getStart(native.source)), trailing = native.owner(x.end);
    uniform(native, x.getStart(native.source), x.end, entry, 'original loop binding');
    uniform(actual, y.getStart(actual.source), y.end, entry, 'hygienic loop binding');
    ranges.get(native).at(-1).trailing = trailing; ranges.get(actual).at(-1).trailing = trailing;
  }
  function value(x, y) {
    if (paired.get(x)?.has(y)) return;
    if (!paired.has(x)) paired.set(x, new Set()); paired.get(x).add(y);
    assert.equal(y.kind, x.kind, 'complete expression role'); assert.deepEqual(y.entry, x.entry, 'expression entry');
    assert.equal(y.phase, x.phase, 'same side of the finally boundary');
    if (y.peer) assert.equal(y.peer, x, 'consistent carried value'); y.peer = x; x.peer = y;
    if (x.kind === 'this') {
      pairTokens([native.token(x.node)], [actual.token(y.node)], 'dynamic receiver read');
    } else if (x.kind === 'literal') {
      assert.equal(y.text, x.text); pairTokens([native.token(x.node)], [actual.token(y.node)], 'literal');
    } else if (x.kind === 'receiver') {
      assert.equal(y.text, x.text, 'the same proved lexical receiver');
      const entry = native.owner(x.node.getStart(native.source)), end = native.owner(x.node.end);
      uniform(native, x.node.getStart(native.source), x.node.end, entry, 'Native lexical receiver');
      uniform(actual, y.node.getStart(actual.source), y.node.end, entry, 'captured lexical receiver');
      assert.deepEqual(actual.owner(y.node.end), end, 'lexical receiver exact endpoint');
      ranges.get(native).at(-1).trailing = end; ranges.get(actual).at(-1).trailing = end;
    } else if (x.kind === 'super-member') {
      const proof = require('./ge5_async_super_member_roles.cjs').paired(x.capability, y.capability);
      assert.equal(y.text, x.text, 'same source-bound super lookup');
      for (const [view, node] of [[native, x.node], [actual, y.node]]) {
        for (let offset = node.getStart(view.source); offset < node.end; offset++) assert(view.covered(offset), 'super member remains independently proved');
        cover(view, node.getStart(view.source), node.end, proof.end);
      }
    } else if (x.kind === 'user') {
      assert.equal(y.text, x.text, 'original user binding');
      assert.equal(y.known, x.known, 'same captured loop value');
      if (n.loop && x.node.text === n.loopBinding.alias) loopBinding(x.node, y.node);
      else pairTokens([native.token(x.node)], [actual.token(y.node)], 'user binding read');
    } else if (x.kind === 'group') {
      const xt = native.tokens(x.node), yt = actual.tokens(y.node);
      pairTokens([xt[0]], [yt[0]], 'original grouping open');
      value(x.inner, y.inner);
      pairTokens([xt.at(-1)], [yt.at(-1)], 'original grouping close');
    } else if (x.kind === 'binary' || x.kind === 'compound-assignment') {
      assert.equal(y.operator, x.operator);
      value(x.left, y.left);
      pairTokens([native.token(x.node.operatorToken)], [actual.token(y.node.operatorToken)], 'binary operator');
      value(x.right, y.right);
    } else if (x.kind === 'sent') {
      assert.equal(y.id, x.id, 'resume the correct yield');
      pairTokens(callTokens(native, x, n.context), callTokens(actual, y, a.context), 'resumed value');
      assert.deepEqual(actual.owner(y.node.end), native.owner(x.node.end), 'resumed value endpoint');
    } else if (x.kind === 'exception') {
      value(x.thrown, y.thrown);
      uniform(native, x.node.getStart(native.source), x.node.end, null, 'Native catch transport');
      uniform(actual, y.node.getStart(actual.source), y.node.end, null, 'Blue catch transport');
    } else if (x.kind === 'catch-read') {
      value(x.payload, y.payload);
      const trailing = native.owner(x.node.end);
      uniform(native, x.node.getStart(native.source), x.node.end, x.entry, 'Native caught binding read');
      uniform(actual, y.node.getStart(actual.source), y.node.end, x.entry, 'Blue caught binding read');
      assert.deepEqual(actual.owner(y.node.end), trailing, 'caught binding read endpoint');
      ranges.get(native).at(-1).trailing = trailing; ranges.get(actual).at(-1).trailing = trailing;
    } else if (x.kind === 'property') {
      assert.equal(y.name, x.name, 'original property name'); value(x.object, y.object);
      const tail = (view, item) => view.tokens(item.node).filter(token => token.start >= item.node.expression.end);
      pairTokens(tail(native, x), tail(actual, y), 'property access');
    } else if(x.kind==='array') {
      assert.equal(y.elements.length,x.elements.length,'complete array inventory');
      for(let index=0;index<x.elements.length;index++)value(x.elements[index],y.elements[index]);
      const punctuation=(view,item)=>view.tokens(item.node).filter(token=>!item.node.elements.some(element=>token.start>=element.getStart(view.source)&&token.end<=element.end));
      pairTokens(punctuation(native,x),punctuation(actual,y),'array punctuation');
    } else if(x.kind==='element') {
      assert.equal(y.reference,x.reference,'same computed access role');value(x.object,y.object);value(x.key,y.key);
      const punctuation=(view,item)=>view.tokens(item.node).filter(token=>token.start>=item.node.expression.end&&!(token.start>=item.node.argumentExpression.getStart(view.source)&&token.end<=item.node.argumentExpression.end));
      pairTokens(punctuation(native,x),punctuation(actual,y),'computed reference punctuation');
    } else if (x.kind === 'call') {
      value(x.callee, y.callee); assert.equal(y.arguments.length, x.arguments.length, 'complete argument inventory');
      for (let index = 0; index < x.arguments.length; index++) value(x.arguments[index], y.arguments[index]);
      if (x.dispatch || y.dispatch) {
        assert(x.dispatch && y.dispatch, 'reviewed captured-call ABI');
        const na = x.dispatch.array.elements, aa = y.dispatch.array.elements; assert(na.length > 0);
        uniform(native, x.node.getStart(native.source), na[0].getStart(native.source), x.entry, 'Native captured-call prefix');
        for (let index = 1; index < na.length; index++) {
          uniform(native, na[index - 1].end, na[index].getStart(native.source), native.owner(na[index - 1].end), 'Native captured argument separator');
        }
        const end = native.owner(na.at(-1).end);
        uniform(native, na.at(-1).end, x.node.end, end, 'Native captured-call endpoint');
        ranges.get(native).at(-1).trailing = native.owner(x.node.end);
        actual.noSource(y.node.getStart(actual.source), y.node.end - 1, 'generated captured-call transport');
        uniform(actual, y.node.end - 1, y.node.end, end, 'captured-call logical close');
        ranges.get(actual).at(-1).trailing = native.owner(x.node.end);
        return;
      }
      const punctuation = (view, item) => view.tokens(item.node).filter(token => token.start >= item.node.expression.end
        && !item.node.arguments.some(argument => token.start >= argument.getStart(view.source) && token.end <= argument.end));
      pairTokens(punctuation(native, x), punctuation(actual, y), 'call punctuation');
    } else assert.fail('unreviewed expression role ' + x.kind);
  }
  for (let path = 0; path < nativePaths.length; path++) {
  const nt = nativePaths[path], at = actualPaths[path]; assert.deepEqual(at.choices, nt.choices);
  assert.equal(at.events.length, nt.events.length, 'complete ordered completion/yield/store/branch trace');
  for (let index = 0; index < nt.events.length; index++) {
    const x = nt.events[index], y = at.events[index]; assert.equal(y.kind, x.kind);
    if (x.kind !== 'jump') value(x.value, y.value);
    if (x.kind === 'yield') assert.deepEqual(actual.owner(y.value.node.end), native.owner(x.value.node.end), 'complete source-bound yield operand endpoint');
    if (x.kind === 'case') {
      assert.equal(y.chosen, x.chosen, 'same ordered switch selection');
      if (y.projection) {
        require('./ge5_generator_switch_machine.cjs').compareCase(ts, n, x, a, y, {value, uniform, cover});
        continue;
      }
      value(x.test, y.test);
      const header = (view, event) => view.tokens(event.clause)
        .filter(token => token.end <= event.opcode.getStart(view.source));
      pairTokens(header(native, x), header(actual, y), 'source case header');
      const prefix = native.owner(x.opcode.getStart(native.source)); assert(prefix);
      const trailing = native.owner(x.opcode.end);
      for (const [view, event] of [[native, x], [actual, y]]) {
        const clauses = event.statement.caseBlock.clauses;
        view.noSource(event.statement.getStart(view.source), clauses[0].getStart(view.source), 'switch selector transport');
        view.noSource(event.statement.caseBlock.end - 1, event.statement.caseBlock.end, 'switch dispatch close');
        uniform(view, event.opcode.getStart(view.source), event.opcode.end, prefix, 'source case jump');
        assert.deepEqual(view.owner(event.opcode.end), trailing, 'source case jump endpoint');
        ranges.get(view).at(-1).trailing = trailing;
      }
      continue;
    }
    if (x.kind === 'jump') {
      const prefix = native.owner(x.statement.getStart(native.source)); assert(prefix);
      const trailing = native.owner(x.statement.end);
      uniform(native, x.statement.getStart(native.source), x.statement.end, prefix, 'Native source break');
      uniform(actual, y.statement.getStart(actual.source), y.statement.end, prefix, 'Blue source break');
      assert.deepEqual(actual.owner(y.statement.end), trailing, 'source break endpoint');
      ranges.get(native).at(-1).trailing = trailing; ranges.get(actual).at(-1).trailing = trailing;
      continue;
    }
    if (x.kind === 'resume') {
      const endpoint = native.owner(x.value.node.end), trailing = native.owner(x.statement.end);
      uniform(native, x.value.node.end, x.statement.end, endpoint, 'Native discarded resume endpoint');
      uniform(actual, y.value.node.end, y.statement.end, endpoint, 'Blue discarded resume endpoint');
      assert.deepEqual(actual.owner(y.statement.end), trailing, 'discarded resume statement end');
      ranges.get(native).at(-1).trailing = trailing; ranges.get(actual).at(-1).trailing = trailing;
      continue;
    }
    if (x.kind === 'effect') {
      const endpoint = native.owner(x.value.node.end), trailing = native.owner(x.statement.end);
      uniform(native, x.value.node.end, x.statement.end, endpoint, 'Native effect endpoint');
      uniform(actual, y.value.node.end, y.statement.end, endpoint, 'Blue effect endpoint');
      assert.deepEqual(actual.owner(y.statement.end), trailing, 'effect statement endpoint');
      ranges.get(native).at(-1).trailing = trailing; ranges.get(actual).at(-1).trailing = trailing;
      continue;
    }
    if (x.kind === 'delegate') {
      assert(x.delegateWrapper && !y.delegateWrapper, 'Native iterable adaptation and Blue delegate action');
      const keyword = native.owner(x.statement.getStart(native.source));
      assert.deepEqual(native.owner(x.delegateWrapper.getStart(native.source)), keyword, 'Native delegate adapter inherits its yield-star operation');
      uniform(native, x.delegateWrapper.getStart(native.source), x.operand.getStart(native.source), keyword, 'Native delegate adapter prefix');
      const ending = native.owner(x.operand.end);
      uniform(native, x.operand.end, x.delegateWrapper.end, ending, 'Native delegate adapter endpoint');
      ranges.get(native).at(-1).trailing = native.owner(x.delegateWrapper.end);
    }
    if (x.kind === 'store') {
      if (n.loop) {
        loopBinding(x.left, y.left);
        pairTokens([native.token(x.assignment.operatorToken)], [actual.token(y.assignment.operatorToken)], 'loop initializer operation');
      } else {
        assert.equal(y.left.text, x.left.text, 'original write target');
        pairTokens([native.token(x.left), native.token(x.assignment.operatorToken)],
          [actual.token(y.left), actual.token(y.assignment.operatorToken)], 'user initializer assignment');
      }
      const endpoint = native.owner(x.right.end);
      uniform(native, x.right.end, x.statement.end, endpoint, 'Native initializer endpoint');
      uniform(actual, y.statement.end - 1, y.statement.end, endpoint, 'Blue initializer endpoint');
      assert.deepEqual(actual.owner(y.statement.end), native.owner(x.statement.end), 'initializer statement end');
      ranges.get(native).at(-1).trailing = native.owner(x.statement.end);
      ranges.get(actual).at(-1).trailing = native.owner(x.statement.end);
      continue;
    }
    if (x.kind === 'update') {
      assert.equal(y.operator, x.operator); assert.equal(y.postfix, x.postfix);
      assert.deepEqual([y.before, y.after], [x.before, x.after], 'same loop update');
      const tail = (view, event) => view.tokens(event.statement).filter(token => token.start >= event.value.node.end);
      pairTokens(tail(native, x), tail(actual, y), 'loop update operator and terminator');
      assert.deepEqual(actual.owner(y.statement.end), native.owner(x.statement.end), 'loop update statement endpoint');
      continue;
    }
    if(x.kind==='write') {
      value(x.reference,y.reference);
      pairTokens([native.token(x.assignment.operatorToken)],[actual.token(y.assignment.operatorToken)],'computed write operation');
      const endpoint=native.owner(x.right.end),trailing=native.owner(x.statement.end);
      uniform(native,x.right.end,x.statement.end,endpoint,'Native computed write endpoint');
      uniform(actual,y.statement.end-1,y.statement.end,endpoint,'computed write terminator');
      assert.deepEqual(actual.owner(y.statement.end),trailing,'computed write statement endpoint');
      ranges.get(native).at(-1).trailing=trailing;ranges.get(actual).at(-1).trailing=trailing;continue;
    }
    if (x.kind === 'branch') {
      assert.equal(y.chosen, x.chosen, 'same normalized branch choice');
      const end = native.owner(x.value.node.end);
      if (n.loop) {
        const condition = y.statement.expression.right; assert(ts.isConditionalExpression(condition));
        if (x.value.operator === '<') {
          const nativeReturn = nt.events.at(-1), actualReturn = at.events.at(-1);
          assert.equal(nativeReturn.kind, 'return'); assert.equal(actualReturn.kind, 'return');
          const ending = (machine, statement) => [...machine.states].find(([, statements]) => statements.includes(statement))[0];
          assert.equal(Number(x.opcode.expression.elements[1].text), ending(n, nativeReturn.statement), 'Native guard exits the loop');
          assert.equal(Number(condition.whenFalse.getText()), ending(a, actualReturn.statement), 'Blue guard exits the loop');
          const owner = native.owner(x.opcode.getStart(native.source)); assert(owner);
          uniform(native, x.opcode.getStart(native.source), x.opcode.end, owner, 'Native loop exit');
          uniform(actual, condition.whenFalse.getStart(actual.source), condition.whenFalse.end, owner, 'Blue loop exit');
          ranges.get(native).at(-1).trailing = native.owner(x.opcode.end);
          actual.noSource(y.statement.getStart(actual.source), condition.whenFalse.getStart(actual.source), 'loop dispatch before exit');
          actual.noSource(condition.whenFalse.end, y.statement.end, 'loop dispatch after exit');
          for (let offset = x.statement.getStart(native.source); offset < x.opcode.getStart(native.source); offset++) {
            if (native.covered(offset)) continue;
            if (offset >= x.value.node.getStart(native.source) && offset < x.value.node.end) continue;
            const current = native.owner(offset);
            assert(current === null || offset < x.value.node.getStart(native.source)
              && JSON.stringify(current) === JSON.stringify(x.value.entry) && /[(!\s]/.test(native.code[offset])
              || offset >= x.value.node.end && JSON.stringify(current) === JSON.stringify(end) && /[)\s]/.test(native.code[offset]),
              'Native generated loop-guard grouping at ' + offset);
            cover(native, offset, offset + 1, end);
          }
        } else {
          assert.equal(x.value.operator, '===');
          const prefix = native.owner(x.statement.getStart(native.source)); assert(prefix);
          uniform(native, x.statement.getStart(native.source), x.value.node.getStart(native.source), prefix, 'original if prefix');
          uniform(native, x.value.node.end, x.opcode.getStart(native.source), end, 'original if close', true);
          const definition = at.definitions.find(definition => definition.value === y.value); assert(definition);
          predicates.set(definition.value, { prefix, end });
          actual.noSource(y.statement.getStart(actual.source), y.statement.end, 'captured if dispatch');
        }
        continue;
      }
      for (let offset = x.statement.getStart(native.source); offset < x.opcode.getStart(native.source); offset++) {
        if (native.covered(offset)) continue;
        const owner = native.owner(offset);
        assert(owner === null || offset >= x.value.node.end && JSON.stringify(owner) === JSON.stringify(end)
          && /[\s)]/.test(native.code[offset]), 'Native generated branch-test boundary');
        cover(native, offset, offset + 1, end);
      }
      uniform(native, x.opcode.getStart(native.source), x.opcode.end, x.value.entry, 'Native logical jump opcode');
      ranges.get(native).at(-1).trailing = native.owner(x.opcode.end);
      continue;
    }
    const prefix = native.owner(x.statement.getStart(native.source)); assert(prefix);
    uniform(native, x.statement.getStart(native.source), x.root.getStart(native.source), prefix, 'Native opcode prefix');
    uniform(actual, y.statement.getStart(actual.source), y.root.getStart(actual.source), prefix, 'Blue opcode prefix');
    const endpoint = native.owner(x.root.end); assert(endpoint);
    uniform(native, x.root.end, x.statement.end, endpoint, 'Native opcode endpoint');
    uniform(actual, y.root.end - 1, y.statement.end, endpoint, 'Blue opcode endpoint');
    if (x.kind === 'return' || x.kind === 'throw') assert.deepEqual(actual.owner(y.statement.end), native.owner(x.statement.end), 'completion statement endpoint');
    ranges.get(native).at(-1).trailing = native.owner(x.statement.end);
    ranges.get(actual).at(-1).trailing = native.owner(x.statement.end);
  }
  if(n.computed || n.superMember || a.superMember) {
    assert.equal(at.reads.length,nt.reads.length,'complete original binding read inventory');
    for(let index=0;index<nt.reads.length;index++){const x=nt.reads[index],y=at.reads[index];assert.equal(y.value.peer,x.value,'same captured read');assert.deepEqual([y.yielded,y.resumed],[x.yielded,x.resumed],'binding read stays on its original side of each suspension');}
  }
  for (const [view, trace, blue] of [[native, nt, false], [actual, at, true]]) {
    for (const item of trace.values) {
      assert(item.peer, 'no discarded/unreviewed expression evaluation: ' + item.node.getText());
      const expected = blue ? item.peer : item, entry = expected.entry;
      for (const use of item.uses) uniform(view, use.getStart(view.source), use.end, entry, 'carried value source');
      for(const {node,reference}of item.referenceUses) {
        const event=trace.events.find(event=>event.kind==='write'&&event.left===reference);assert(event,'reference alias belongs to a computed write');
        const counterpart=blue?nt.events[trace.events.indexOf(event)]:event;
        uniform(view,node.getStart(view.source),node.end,counterpart.reference.entry,'captured computed reference source');
      }
      for (const { node, binding } of item.initializerUses) {
        const store = trace.events.find(event => event.kind === 'store' && event.left === binding);
        assert(store, 'initializer transport belongs to a user store');
        const counterpart = blue ? nt.events[trace.events.indexOf(store)] : store;
        uniform(view, node.getStart(view.source), node.end, native.owner(counterpart.right.getStart(native.source)), 'carried stored value');
      }
      for (const { node, statement } of item.returnUses) {
        const event = trace.events.find(event => event.statement === statement);
        assert(event, 'carried return value belongs to its completion');
        const counterpart = blue ? nt.events[trace.events.indexOf(event)] : event;
        uniform(view, node.getStart(view.source), node.end, native.owner(counterpart.operand.getStart(native.source)), 'carried return value');
      }
    }
    for (const definition of trace.definitions) {
      if (trace.machine.exceptions === 'switch-delegate-finally'
        && require('./ge5_generator_switch_machine.cjs').caseDefinition(trace.machine, definition.statement)) {
        for (let at = definition.statement.getStart(view.source); at < definition.statement.end; at++) {
          assert(view.covered(at), 'source-case capture was independently and completely classified');
        }
        continue;
      }
      const expected = blue ? definition.value.peer : definition.value;
      const start = definition.statement.getStart(view.source), first = definition.right.getStart(view.source);
      if (blue && predicates.has(definition.value)) uniform(view, start, first, predicates.get(definition.value).prefix, 'captured original if prefix');
      else if (blue) view.noSource(start, first, 'generated slot assignment');
      else uniform(view, start, first, expected.entry, 'Native slot prefix');
      const end = native.owner(expected.node.end);
      uniform(view, definition.right.end, definition.statement.end, end, 'temporary definition endpoint',
        !(blue && predicates.has(definition.value)));
      if (blue && predicates.has(definition.value)) {
        assert.deepEqual(view.owner(definition.statement.end), predicates.get(definition.value).end, 'captured if endpoint');
      }
    }
    for (const control of trace.controls) view.noSource(control.getStart(view.source), control.end, 'continuation dispatch');
    for (const { node, value: item, template, concise } of trace.generatedGroups) {
      const expected = blue ? item.peer : item; assert(expected, 'reviewed precedence grouping');
      const parent = node.parent;
      const entry = blue ? template || concise ? native.owner(expected.node.getStart(native.source)) : null : ts.isBinaryExpression(parent) && parent.right === node
        ? native.owner(parent.operatorToken.getStart(native.source)) : expected.entry;
      uniform(view, node.getStart(view.source), node.getStart(view.source) + 1, entry, 'generated precedence open');
      uniform(view, node.end - 1, node.end, native.owner(expected.node.end), 'generated precedence close');
      ranges.get(view).at(-1).trailing = native.owner(blue ? expected.node.end : node.end);
    }
  }
  }
  const header = (view, fn) => view.tokens(fn.node).filter(token => token.end <= fn.node.body.getStart(view.source) + 1);
  if (n.asyncGeneratorAdapter || a.asyncGeneratorAdapter) {
    const signature = require('./ge5_async_generator_machine.cjs').signature(ts, native, n, actual, a);
    cover(native, signature.native[0], signature.native[1], signature.trailing);
    cover(actual, signature.actual[0], signature.actual[1], signature.trailing);
  } else if (n.reviewedSignature) {
    const signature = require('./ge5_relocated_callable_signature.cjs').verify(ts, native, n, actual, a, n.reviewedSignature);
    cover(native, signature.native[0], signature.native[1], signature.trailing);
    cover(actual, signature.actual[0], signature.actual[1], signature.trailing);
  } else if (n.outer) {
    const nh = header(native, { node: n.outer.node }), ah = header(actual, { node: a.outer.node });
    const no = nh.findIndex(token => token.text === '('), ao = ah.findIndex(token => token.text === '(');
    const nc = nh.findLastIndex(token => token.text === ')');
    const callback = header(actual, a), po = callback.findIndex(token => token.text === '('), pc = callback.findLastIndex(token => token.text === ')');
    pairTokens(nh.slice(0, no), ah.slice(0, ao), 'async function identity');
    pairTokens(nh.slice(no, nc + 1), callback.slice(po, pc + 1), 'moved async signature');
    pairTokens([nh.at(-1)], [ah.at(-1)], 'async body entry');
    pairTokens([nh.at(-1)], [callback.at(-1)], 'moved async body entry');
    native.noSource(n.node.getStart(native.source), n.node.body.getStart(native.source) + 1, 'Native driver callback declaration');
    actual.noSource(ah[ao].start, ah.at(-1).start, 'generated async argument slots');
    actual.noSource(a.outer.returned.getStart(actual.source), callback[po].start, 'generated awaiter setup');
    native.noSource(n.outer.returned.getStart(native.source), n.node.body.getStart(native.source) + 1, 'Native awaiter setup');
    const tail = native.owner(nh[nc].end);
    for (let offset = nh[nc].end; offset < nh.at(-1).start; offset++) assert.deepEqual(native.owner(offset), tail, 'Native signature trivia');
    cover(native, nh[nc].end, nh.at(-1).start, tail);
    uniform(actual, callback[pc].end, callback.at(-1).start, native.owner(nh.at(-1).start), 'moved signature trivia');
  } else pairTokens(header(native, n), header(actual, a), 'complete generator signature');
  const signature = native.owner(n.node.body.getStart(native.source));
  const prologue = n.reviewedPrologue
    ? require('./ge5_async_arrow_prologue.cjs').interval(ts, native, n) : null;
  if (prologue) {
    uniform(native, n.node.body.getStart(native.source) + 1, prologue[0], signature, 'Native signature/prologue boundary', true);
    uniform(native, prologue[1], n.dispatcher.getStart(native.source), signature, 'Native prologue/driver boundary', true);
  } else {
    uniform(native, n.node.body.getStart(native.source) + 1, n.dispatcher.getStart(native.source), signature, 'Native driver signature boundary', true);
  }
  for (const [view, fn] of [[native, n], [actual, a]]) {
    const ordered = ranges.get(view).sort((left, right) => left.start - right.start);
    const boundary = fn.outer?.node ?? fn.node;
    for (let offset = boundary.getStart(view.source); offset < boundary.end; offset++) {
      if (view.covered(offset)) continue;
      const previous = ordered.findLast(role => role.end <= offset), owner = view.owner(offset);
      assert(owner === null || /\s/.test(view.code[offset]) && previous && JSON.stringify(owner) === JSON.stringify(previous.trailing),
        'unclassified source grant at ' + offset + ': ' + JSON.stringify(owner));
      view.cover(offset, offset + 1);
    }
  }
  return { native: nativePaths[0], actual: actualPaths[0], nativePaths, actualPaths };
}

function compare(ts, native, actual, originals, options = {}) {
  assert.deepEqual(actual.decoded.metadata, native.decoded.metadata);
  function parts(view, dialect) {
    const result = [], helpers = []; let functions = 0;
    for (const node of view.source.statements) {
      const name = ts.isFunctionDeclaration(node) ? node.name?.text : ts.isVariableStatement(node)
        && node.declarationList.declarations.length === 1 && node.declarationList.declarations[0].name.getText();
      if (name === (options.asynchronous ? 'calculate' : 'sequence') || options.delegation && name === 'child') {
        functions++;
        const wrapped = options.asynchronous ? require('./ge5_async_continuation.cjs').unwrap(ts, view, node, dialect) : null;
        const fn = machine.read(ts, view, wrapped?.callback ?? node, dialect, originals, options); if (wrapped) fn.outer = wrapped.outer;
        result.push({ start: node.getStart(view.source), end: node.end, fn }); continue;
      }
      const helper = dialect === 'native' ? options.delegation ? /^__(?:generator|awaiter|values)(?:_\d+)?$/ : /^__(?:generator|awaiter)(?:_\d+)?$/
        : /^__blueice_target_(?:generator|async_driver|delegate_iterator|delegate_step)_\d+$/;
      if (name && helper.test(name) && (dialect === 'native' || ts.isFunctionDeclaration(node))) {
        assert(!originals.some(source => source.includes(name)), 'fresh generated helper');
        helpers.push(node); view.noSource(node.getFullStart(), node.end, 'complete generated helper'); continue;
      }
      result.push({ start: node.getStart(view.source), end: node.end, tokens: view.tokens(node) });
    }
    if (options.delegation && dialect === 'blue') require('./ge5_generator_abi.cjs').validate(ts, view, helpers, originals);
    assert.equal(functions, options.delegation ? 2 : 1); return result;
  }
  const wanted = parts(native, 'native'), observed = parts(actual, 'blue'); assert.equal(observed.length, wanted.length);
  let result; const functions = {};
  for (let index = 0; index < wanted.length; index++) {
    const n = wanted[index], a = observed[index]; assert.equal(Boolean(a.fn), Boolean(n.fn));
    if (n.fn) {
      result = compareFunction(ts, native, n.fn, actual, a.fn);
      if (options.delegation) functions[n.fn.node.name.text] = result;
    }
    else compareTokens(native, n.tokens, actual, a.tokens, 'module statement ' + index);
    compareGap(native, index ? wanted[index - 1].end : 0, n.start, actual, index ? observed[index - 1].end : 0, a.start, 'module gap');
  }
  compareGap(native, wanted.at(-1).end, native.code.length, actual, observed.at(-1).end, actual.code.length, 'module trailing gap');
  assert.deepEqual(actual.owner(actual.code.length), native.owner(native.code.length));
  native.complete('Native module'); actual.complete('Blue module');
  return options.delegation ? { ...result, functions } : result;
}
module.exports = { compare, compareFunction };
