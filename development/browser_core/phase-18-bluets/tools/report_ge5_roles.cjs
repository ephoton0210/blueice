// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Partial G-E5 evidence. This publisher cannot mark the TODO or full parity done.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process'), { sha256 } = require('./ge5_inputs.cjs');
const root = path.resolve(__dirname, '../../../..'), phase = path.dirname(__dirname);
function argument(name) {
  const index = process.argv.indexOf(name); assert(index > 0 && process.argv[index + 1], name);
  return process.argv[index + 1];
}
const broad = fs.readFileSync(argument('--broad-log'), 'utf8');
const clippy = fs.readFileSync(argument('--clippy-log'), 'utf8');
const debuggerLog = fs.readFileSync(argument('--debugger-log'), 'utf8');
const boundaryLog = fs.readFileSync(argument('--boundary-log'), 'utf8');
const node = JSON.parse(fs.readFileSync(argument('--node-report')));
const native = JSON.parse(fs.readFileSync(argument('--native-report')));
const asynchronous = JSON.parse(fs.readFileSync(argument('--async-report')));
const generators = JSON.parse(fs.readFileSync(argument('--generator-report')));
const continuations = JSON.parse(fs.readFileSync(argument('--continuation-report')));
const linear = JSON.parse(fs.readFileSync(argument('--linear-report')));
const branching = JSON.parse(fs.readFileSync(argument('--branch-report')));
const namedAsync = JSON.parse(fs.readFileSync(argument('--named-async-report')));
const nestedAsync = JSON.parse(fs.readFileSync(argument('--nested-async-report')));
const conditionalAsync = JSON.parse(fs.readFileSync(argument('--conditional-async-report')));
const conditionalStatementAsync = JSON.parse(fs.readFileSync(argument('--conditional-statement-async-report')));
const templateAsync = JSON.parse(fs.readFileSync(argument('--template-async-report')));
const templateGenerator = JSON.parse(fs.readFileSync(argument('--template-generator-report')));
const optionalModules = JSON.parse(fs.readFileSync(argument('--optional-module-report')));
const spreadModules = JSON.parse(fs.readFileSync(argument('--spread-module-report')));
const taggedModules = JSON.parse(fs.readFileSync(argument('--tagged-module-report')));
const computed = JSON.parse(fs.readFileSync(argument('--computed-report')));
const switches = JSON.parse(fs.readFileSync(argument('--switch-report')));
const finallyReturn = JSON.parse(fs.readFileSync(argument('--finally-report')));
const catchFinally = JSON.parse(fs.readFileSync(argument('--catch-finally-report')));
const loopFinally = JSON.parse(fs.readFileSync(argument('--loop-finally-report')));
const delegatedFinally = JSON.parse(fs.readFileSync(argument('--delegate-finally-report')));
const bindingForms = ['array-nested-binding', 'object-nested-binding', 'mixed-nested-default', 'nested-parameter', 'binding-iterator-finally'];
const bindingReports = bindingForms.map(form => JSON.parse(fs.readFileSync(path.join(argument('--binding-report-directory'), 'binding-' + form + '.json'))));
const decoratorPublication = JSON.parse(fs.readFileSync(argument('--decorator-publication-report')));
const objectRest = JSON.parse(fs.readFileSync(argument('--object-report')));
const arrowDefault = JSON.parse(fs.readFileSync(argument('--arrow-default-report')));
const lexicalArrow = JSON.parse(fs.readFileSync(argument('--lexical-arrow-report')));
const objectMethod = JSON.parse(fs.readFileSync(argument('--object-method-report')));
const classMethod = JSON.parse(fs.readFileSync(argument('--class-method-report')));
const asyncGenerator = JSON.parse(fs.readFileSync(argument('--async-generator-report')));
const superMethod = JSON.parse(fs.readFileSync(argument('--super-method-report')));
const g2Generator = JSON.parse(fs.readFileSync(argument('--g2-generator-report')));
const legacyDecorator = JSON.parse(fs.readFileSync(argument('--legacy-decorator-report')));
const iteration = JSON.parse(fs.readFileSync(argument('--iteration-report')));
const classSuper = JSON.parse(fs.readFileSync(argument('--class-report')));
const classExecution = JSON.parse(fs.readFileSync(argument('--class-execution-report')));
const classSyntax = JSON.parse(fs.readFileSync(argument('--class-syntax-report')));
const privateForms = ['private-super-method', 'private-static-super', 'private-super-accessors', 'private-super-async-generator'];
const privateReports = privateForms.map(form => JSON.parse(fs.readFileSync(path.join(argument('--private-report-directory'), 'decorator-' + form + '.json'))));
const decoratorClassPhase = JSON.parse(fs.readFileSync(argument('--decorator-class-phase-report')));
const behaviorBaselineBytes = process.argv.includes('--behavior-baseline') ? fs.readFileSync(argument('--behavior-baseline')) : null;
const behaviorBaseline = behaviorBaselineBytes ? JSON.parse(behaviorBaselineBytes) : null;
const roleLog = fs.readFileSync(argument('--role-log'), 'utf8');
const toolsLog = fs.readFileSync(argument('--tools-log'), 'utf8');
const runtimeLog = fs.readFileSync(argument('--runtime-log'), 'utf8');
const localLog = fs.readFileSync(argument('--local-log'), 'utf8');
const directory = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps');
const inputBytes = fs.readFileSync(path.join(directory, 'cases.json')), input = JSON.parse(inputBytes);
const referenceBytes = fs.readFileSync(path.join(directory, 'reference.json')), reference = JSON.parse(referenceBytes);
assert.equal(sha256(inputBytes), '5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e');
assert.equal(sha256(referenceBytes), '166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3');
assert(broad.includes('test javascript_roles::all_non_scalar_declarations_and_signature_regressions_keep_native_javascript_roles ... ok'));
assert(broad.includes('test node_roles::all_node_module_products_preserve_complete_token_owners_and_generated_gaps ... ok'));
assert(broad.includes('test native_defects::native_defects_all_572_have_bounded_bluets_maps_and_unmapped_execute_braces ... ok'));
assert(!broad.includes('FAILED')); assert(clippy.includes('Finished')); assert(!clippy.includes('error:'));
assert(debuggerLog.includes('test result: ok. 22 passed; 0 failed; 0 ignored;'));
assert(!boundaryLog.includes('FAILED'));
for (const test of ['javascript_boundaries::es5_async_resume_binding_and_return_opcode_keep_distinct_native_origins',
  'javascript_boundaries::es5_template_await_interpolation_preserves_the_original_operand',
  'javascript_boundaries::es5_tagged_raw_spans_and_completion_keep_distinct_native_owners',
  'javascript_boundaries::es5_computed_write_keeps_saved_reference_and_statement_end_owners',
  'javascript_boundaries::es5_switch_case_jumps_and_break_keep_distinct_native_owners',
  'javascript_boundaries::es5_finally_pending_and_overriding_returns_keep_distinct_native_owners',
  'javascript_boundaries::es5_caught_alias_and_effects_keep_native_owners_without_transport_grants',
  'javascript_boundaries::es5_loop_initializer_predicates_and_exit_keep_distinct_native_owners',
  'javascript_boundaries::es5_named_class_publication_retains_the_native_name_span',
  'javascript_boundaries::system_class_publication_is_generated_transport',
  'boundaries::helper_collision_and_cache_provenance']) assert(boundaryLog.includes('test ' + test + ' ... ok'));
const format = spawnSync('cargo', ['fmt', '--all', '--', '--check'], { cwd: root, encoding: 'utf8' });
assert.equal(format.status, 0, format.stdout + format.stderr);
const compilerSha256 = sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc')));
assert(fs.readFileSync(path.join(root, 'backend/bluets/src/compiler/fingerprint.rs'), 'utf8').includes('bluets-javascript-provenance/20'));
assert(fs.readFileSync(path.join(root, 'backend/bluets/src/bin/bluetsc/build_graph/freshness.rs'), 'utf8').includes('const STATE_VERSION: u8 = 45;'));
assert.equal(node.compilerSha256, compilerSha256); assert.equal(native.bluetsBinarySha256, compilerSha256);
for (const report of [asynchronous, generators]) {
  assert.equal(report.compilerSha256, compilerSha256);
  assert.deepEqual(report.failures, []); assert.equal(report.repeatBuilds, 2);
  assert.equal(report.completeFamilies, false);
  assert(report.verified.every(row => row.allCharactersClassified));
}
assert.equal(asynchronous.configurations, 100); assert.equal(asynchronous.maps, 100);
assert.equal(generators.configurations, 200); assert.equal(generators.maps, 200);
assert.equal(continuations.compilerSha256, compilerSha256);
assert.equal(continuations.repeatBuilds, 2); assert.equal(continuations.configurations, 7);
assert.equal(continuations.maps, 7); assert.deepEqual(continuations.failures, []);
assert.equal(continuations.verified.length, 7);
assert(continuations.verified.every(row => row.allCharactersClassified));
for (const [report, form, paths, controls] of [[linear, 'binary-yield-order', 1, 98], [branching, 'logical-yield-order', 4, 77],
  [namedAsync, 'helper-name-collisions', 1, 84], [nestedAsync, 'nested-await-call', 1, 98],
  [conditionalAsync, 'conditional-expression-await', 2, 91],
  [conditionalStatementAsync, 'conditional-await', 2, 126],
  [templateAsync, 'template-await-order', 1, 147], [templateGenerator, 'template-yield-order', 1, 140]]) {
  assert.equal(report.compilerSha256, compilerSha256); assert.equal(report.repeatBuilds, 2);
  assert.equal(report.form, form); assert.equal(report.configurations, 7); assert.equal(report.maps, 7);
  assert.equal(report.verified.length, 7); assert.deepEqual(report.failures, []);
  assert(report.verified.every(row => row.allCharactersClassified && row.abstractPaths === paths));
  assert.equal(report.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), controls);
}
assert.equal(continuations.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 70);
assert.equal(optionalModules.compilerSha256, compilerSha256);
assert.equal(optionalModules.repeatBuilds, 2); assert.equal(optionalModules.form, 'optional-nullish');
assert.equal(optionalModules.configurations, 363); assert.equal(optionalModules.maps, 363);
assert.equal(optionalModules.verified.length, 363); assert.deepEqual(optionalModules.failures, []);
assert(optionalModules.verified.every(row => row.allCharactersClassified && row.runtime.actualResult === 42 && row.runtime.nativeResult === 42
  && row.runtime.variants.length === 3 && row.runtime.variants[0].result === 42
  && row.runtime.variants.slice(1).every(variant => variant.result === 0)));
assert.equal(optionalModules.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 8515);
const moduleExecution = { originalArtifactExecutions: optionalModules.verified.reduce((sum, row) => sum + row.runtime.executions, 0),
  additionalNullishFixtureExecutions: optionalModules.verified.reduce((sum, row) => sum + row.runtime.additionalFixtureExecutions, 0) };
assert.deepEqual(moduleExecution, { originalArtifactExecutions: 806, additionalNullishFixtureExecutions: 1612 });
assert.equal(spreadModules.compilerSha256, compilerSha256); assert.equal(spreadModules.repeatBuilds, 2);
assert.equal(spreadModules.form, 'sparse-array-call-spread'); assert.equal(spreadModules.configurations, 363);
assert.equal(spreadModules.maps, 363); assert.equal(spreadModules.verified.length, 363); assert.deepEqual(spreadModules.failures, []);
assert(spreadModules.verified.every(row => row.allCharactersClassified && row.runtime.actualResult === 44 && row.runtime.nativeResult === 44
  && row.runtime.variants.length === 2 && row.runtime.variants[0].result === 44 && row.runtime.variants[1].result === '[3,true,null]'));
assert.equal(spreadModules.mutations.reduce((sum,row) => sum + row.controls.filter(control => control.rejected).length, 0), 9397);
const spreadExecution = { originalArtifactExecutions: spreadModules.verified.reduce((sum,row) => sum + row.runtime.executions, 0),
  additionalSparseFixtureExecutions: spreadModules.verified.reduce((sum,row) => sum + row.runtime.additionalFixtureExecutions, 0) };
assert.deepEqual(spreadExecution, { originalArtifactExecutions: 806, additionalSparseFixtureExecutions: 806 });
assert.equal(taggedModules.compilerSha256, compilerSha256); assert.equal(taggedModules.repeatBuilds, 2);
assert.equal(taggedModules.form, 'tagged-template'); assert.equal(taggedModules.configurations, 363);
assert.equal(taggedModules.maps, 363); assert.equal(taggedModules.verified.length, 363); assert.deepEqual(taggedModules.failures, []);
assert(taggedModules.verified.every(row => row.allCharactersClassified && row.runtime.actualResult === 42 && row.runtime.nativeResult === 42
  && row.runtime.variants.length === 2 && row.runtime.variants[0].result === 42
  && JSON.stringify(JSON.parse(row.runtime.variants[1].result).slice(0,9)) === '[true,2,"a","","a","",false,false,false]'
  && JSON.parse(row.runtime.variants[1].result)[9] === !reference.blobs[row.nativeJavaScriptSha256].includes('__makeTemplateObject')));
assert.equal(taggedModules.mutations.reduce((sum,row) => sum + row.controls.filter(control => control.rejected).length, 0), 8533);
const taggedExecution = { originalArtifactExecutions: taggedModules.verified.reduce((sum,row) => sum + row.runtime.executions, 0),
  additionalRepeatedSiteFixtureExecutions: taggedModules.verified.reduce((sum,row) => sum + row.runtime.additionalFixtureExecutions, 0) };
assert.deepEqual(taggedExecution, { originalArtifactExecutions: 806, additionalRepeatedSiteFixtureExecutions: 806 });
assert.equal(computed.compilerSha256, compilerSha256); assert.equal(computed.repeatBuilds, 2);
assert.equal(computed.form, 'computed-assignment-yield'); assert.equal(computed.configurations, 7); assert.equal(computed.maps, 7);
assert.equal(computed.verified.length, 7); assert.deepEqual(computed.failures, []);
assert(computed.verified.every(row=>row.allCharactersClassified && row.abstractPaths===1 && row.runtime.variants.length===4));
assert.equal(computed.mutations.reduce((sum,row)=>sum+row.controls.filter(control=>control.rejected).length,0),238);
const computedExecution={originalArtifactExecutions:computed.verified.reduce((sum,row)=>sum+row.runtime.executions,0),
  additionalReferenceFixtureExecutions:computed.verified.reduce((sum,row)=>sum+row.runtime.additionalFixtureExecutions,0)};
assert.deepEqual(computedExecution,{originalArtifactExecutions:14,additionalReferenceFixtureExecutions:42});
assert.equal(switches.compilerSha256, compilerSha256); assert.equal(switches.repeatBuilds, 2);
assert.equal(switches.form, 'switch-suspension-fallthrough'); assert.equal(switches.configurations, 7); assert.equal(switches.maps, 7);
assert.equal(switches.verified.length, 7); assert.deepEqual(switches.failures, []);
assert(switches.verified.every(row => row.allCharactersClassified && row.abstractPaths === 3 && row.runtime.variants.length === 6));
assert.equal(switches.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 301);
const switchExecution = { originalArtifactExecutions: switches.verified.reduce((sum, row) => sum + row.runtime.executions, 0),
  additionalBranchFixtureExecutions: switches.verified.reduce((sum, row) => sum + row.runtime.additionalFixtureExecutions, 0) };
assert.deepEqual(switchExecution, { originalArtifactExecutions: 14, additionalBranchFixtureExecutions: 70 });
for (const [report, form, paths, controls, variants] of [[finallyReturn, 'finally-overrides-return', 1, 294, 8],
  [catchFinally, 'switch-exception-finally', 2, 434, 11]]) {
  assert.equal(report.compilerSha256, compilerSha256); assert.equal(report.repeatBuilds, 2);
  assert.equal(report.form, form); assert.equal(report.configurations, 7); assert.equal(report.maps, 7);
  assert.equal(report.verified.length, 7); assert.deepEqual(report.failures, []);
  assert(report.verified.every(row => row.allCharactersClassified && row.abstractPaths === paths && row.runtime.variants.length === variants));
  assert.equal(report.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), controls);
}
const exceptionExecution = { originalArtifactExecutions: [finallyReturn, catchFinally].reduce((sum, report) => sum + report.verified.reduce((n, row) => n + row.runtime.executions, 0), 0),
  additionalAbruptFixtureExecutions: [finallyReturn, catchFinally].reduce((sum, report) => sum + report.verified.reduce((n, row) => n + row.runtime.additionalFixtureExecutions, 0), 0) };
assert.deepEqual(exceptionExecution, { originalArtifactExecutions: 28, additionalAbruptFixtureExecutions: 238 });
let behaviorEvidenceReuse = null;
if (behaviorBaseline) {
assert.equal(behaviorBaseline.compilerSha256, compilerSha256);
const productionFiles = Object.entries(behaviorBaseline.sourceFiles).filter(([file]) => file.startsWith('backend/bluets/src/') || file.startsWith('backend/bluets-bluejs/src/'));
assert(productionFiles.length > 0);
for (const [file, expected] of productionFiles) assert.equal(sha256(fs.readFileSync(path.join(root, file))), expected, file + ': production code changed');
assert.equal(sha256(runtimeLog), behaviorBaseline.gates.runtimeLogSha256);
assert.equal(sha256(localLog), behaviorBaseline.gates.localLogSha256);
assert.equal(sha256(debuggerLog), behaviorBaseline.gates.debuggerLogSha256);
behaviorEvidenceReuse = { compilerSha256, priorMeasurementSha256: sha256(behaviorBaselineBytes),
  verifiedProductionFiles: productionFiles.length, unchangedProductionSource: true,
  ge1RuntimeLogSha256: sha256(runtimeLog), localLogSha256: sha256(localLog), debuggerLogSha256: sha256(debuggerLog) };
}

assert.equal(loopFinally.compilerSha256, compilerSha256); assert.equal(loopFinally.repeatBuilds, 2);
assert.equal(loopFinally.form, 'loop-abrupt-finally'); assert.equal(loopFinally.configurations, 7); assert.equal(loopFinally.maps, 7);
assert.equal(loopFinally.verified.length, 7); assert.deepEqual(loopFinally.failures, []);
assert(loopFinally.verified.every(row => row.allCharactersClassified && row.abstractPaths === 1 && row.runtime.variants.length === 10));
assert.equal(loopFinally.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 364);
const loopExecution = { originalArtifactExecutions: loopFinally.verified.reduce((sum, row) => sum + row.runtime.executions, 0),
  additionalLoopFixtureExecutions: loopFinally.verified.reduce((sum, row) => sum + row.runtime.additionalFixtureExecutions, 0) };
assert.deepEqual(loopExecution, { originalArtifactExecutions: 14, additionalLoopFixtureExecutions: 126 });
assert.equal(delegatedFinally.compilerSha256, compilerSha256);
assert.equal(delegatedFinally.repeatBuilds, 2); assert.equal(delegatedFinally.form, 'delegate-external-return-finally');
assert.equal(delegatedFinally.configurations, 7); assert.equal(delegatedFinally.maps, 7); assert.deepEqual(delegatedFinally.failures, []);
assert.equal(delegatedFinally.verified.length, 7);
assert(delegatedFinally.verified.every(row => row.allCharactersClassified && row.abstractPaths === 1 && row.runtime.variants.length === 10));
assert.equal(delegatedFinally.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 546);
const delegateExecution = { originalArtifactExecutions: delegatedFinally.verified.reduce((sum, row) => sum + row.runtime.executions, 0),
  additionalAbruptFixtureExecutions: delegatedFinally.verified.reduce((sum, row) => sum + row.runtime.additionalFixtureExecutions, 0) };
assert.deepEqual(delegateExecution, { originalArtifactExecutions: 14, additionalAbruptFixtureExecutions: 126 });
for (const [index, report] of bindingReports.entries()) {
  assert.equal(report.compilerSha256, compilerSha256); assert.equal(report.repeatBuilds, 2);
  assert.equal(report.form, bindingForms[index]); assert.equal(report.configurations, index === 0 ? 51 : 27);
  assert.equal(report.maps, report.configurations); assert.equal(report.verified.length, report.maps);
  assert.deepEqual(report.failures, []); assert(report.verified.every(row => row.allCharactersClassified));
}
const bindingExecution = { originalArtifactExecutions: bindingReports.reduce((sum, report) => sum + report.verified.reduce((n, row) => n + row.runtime.executions, 0), 0),
  additionalFixtureExecutions: bindingReports.reduce((sum, report) => sum + report.verified.reduce((n, row) => n + row.runtime.additionalFixtureExecutions, 0), 0) };
assert.deepEqual(bindingExecution, { originalArtifactExecutions: 318, additionalFixtureExecutions: 798 });
assert.equal(bindingReports.reduce((sum, report) => sum + report.verified.reduce((n, row) => n + row.controls.filter(control => control.rejected).length, 0), 0), 13812);
assert.equal(decoratorPublication.compilerSha256, compilerSha256); assert.deepEqual(decoratorPublication.failures, []);
assert.equal(decoratorPublication.rows.length, 32); assert(decoratorPublication.rows.every(row => row.repeatBuilds === 2));
assert.equal(decoratorPublication.rows.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 56);
function completeReport(report, family, configurations, controls, nested = false) {
  assert.equal(report.compilerSha256, compilerSha256); assert.equal(report.repeatBuilds, 2);
  assert.equal(report.configurations, configurations); assert.equal(report.maps, configurations);
  assert.deepEqual(report.failures, []); assert.equal(report.verified.length, configurations);
  const expected = input.cases.filter(row => row.form === family).map(row => row.id).sort();
  assert.deepEqual(report.verified.map(row => row.id).sort(), expected, family + ': entire immutable family');
  const maps = nested ? report.verified.flatMap(row => row.maps) : report.verified;
  assert.equal(maps.length, configurations); assert(maps.every(row => row.allCharactersClassified));
  assert.equal(maps.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), controls);
}
completeReport(objectRest, 'ge2_module_helpers/object-rest-spread', 351, 12911);
completeReport(iteration, 'ge2_module_helpers/iterable-bindings-forof', 339, 31465);
completeReport(arrowDefault, 'ge1_downlevel/async-arrow-default-block', 27, 619);
assert.equal(arrowDefault.completeFamily, true); assert.equal(arrowDefault.components.length, 27);
assert(arrowDefault.components.every(row => row.physicalRoles.length === 0));
const arrowGuards = arrowDefault.components.filter(row => row.guard).map(row => row.guard);
assert.equal(arrowGuards.length, 7); assert(arrowGuards.every(row => row.complete));
assert.equal(arrowGuards.reduce((sum,row) => sum + row.controls.filter(control => control.rejected).length,0), 91);
assert.equal(arrowGuards.reduce((sum,row) => sum + row.prologue.controls.filter(control => control.rejected).length,0), 35);
completeReport(lexicalArrow, 'ge1_downlevel/async-arrow-lexical-this', 51, 1916);
completeReport(objectMethod, 'ge1_downlevel/async-object-method', 27, 2469);
completeReport(classMethod, 'ge1_downlevel/async-class-method', 51, 5419);
completeReport(asyncGenerator, 'ge1_downlevel/async-generator-exception-template', 27, 1466);
completeReport(superMethod, 'ge1_downlevel/async-super-method', 51, 6741);
completeReport(g2Generator, 'ge2_module_helpers/async-generator-finally', 363, 19572);
assert(superMethod.verified.every(row => row.nativeAndActualSyntax));
assert(g2Generator.verified.every(row => row.syntax.native && row.syntax.actual));
assert(classMethod.verified.every(row => row.nativeAndActualSyntax));
assert(asyncGenerator.verified.every(row => row.syntax.native && row.syntax.actual));
for (const [report, original, supplemental] of [[lexicalArrow, 102, 1224], [objectMethod, 54, 702], [classMethod, 102, 1428], [asyncGenerator, 54, 972], [superMethod, 102, 1326], [g2Generator, 806, 12090]]) {
  assert.equal(report.completeFamily, true);
  assert.equal(report.verified.reduce((sum, row) => sum + row.runtime.executions, 0), original);
  assert.equal(report.verified.reduce((sum, row) => sum + row.runtime.additionalFixtureExecutions, 0), supplemental);
  for (const row of report.verified) for (const key of ['actualJavaScriptSha256', 'actualMapSha256']) assert.match(row[key], /^[a-f0-9]{64}$/);
}
assert([objectMethod, classMethod, superMethod].every(report => report.verified.every(row => row.controls.every(control => control.publicationCoherent))));
completeReport(legacyDecorator, 'ge2_module_helpers/legacy-parameter-decorator', 363, 6805);
assert.equal(legacyDecorator.completeFamily, true); assert.equal(legacyDecorator.probeOnly, false);
assert.equal(legacyDecorator.executions.length, 363);
assert.equal(legacyDecorator.executions.reduce((sum,row) => sum + row.executions,0), 986);

const classForms = {'ge1_downlevel/called-imported-heritage': [51, 102], 'ge1_downlevel/indexed-heritage': [51, 51],
  'ge1_downlevel/computed-constructor-spread': [51, 51], 'ge1_downlevel/constructor-and-super-spread': [51, 51],
  'ge1_downlevel/computed-super-call-order': [51, 51], 'ge1_downlevel/super-write-receiver-order': [43, 43],
  'ge2_module_helpers/inherit-constructor-spread': [363, 363]};
assert.deepEqual(classSuper.forms, classForms); assert.equal(classSuper.complete, true);
assert.equal(classSuper.compilerSha256, compilerSha256); assert.equal(classSuper.repeatBuilds, 2);
assert.equal(classSuper.configurations, 661); assert.equal(classSuper.maps, 712); assert.deepEqual(classSuper.failures, []);
assert.equal(classSuper.verified.length, 712); assert(classSuper.verified.every(row => row.allCharactersClassified));
const classMaps = new Map(classSuper.verified.map(row => [row.index + ':' + row.map, row]));
for (const row of classSuper.verified) {
  for (const key of ['nativeJavaScriptSha256', 'nativeStockMapSha256', 'nativeMapSha256', 'actualJavaScriptSha256', 'actualMapSha256']) {
    assert.match(row[key], /^[a-f0-9]{64}$/, 'complete class artifact binding: ' + key);
  }
}
const classIds = input.cases.filter(row => Object.hasOwn(classForms, row.form)).map(row => row.id).sort();
assert.deepEqual([...new Set(classSuper.verified.map(row => row.id))].sort(), classIds);
assert.equal(classSuper.mutations.length, 712);
assert.equal(classSuper.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 8269);
for (const report of [classExecution, classSyntax]) {
  assert.equal(report.compilerSha256, compilerSha256); assert.equal(report.configurations, 661);
  assert.deepEqual([...new Set(report.verified.map(row => row.index))].sort((a,b) => a-b),
    input.cases.flatMap((row,index) => Object.hasOwn(classForms,row.form) ? [index] : []));
}
assert.deepEqual(classExecution.failures, []); assert.equal(classExecution.executions, 1402);
assert.equal(classSyntax.programs, 1424); assert.equal(classSyntax.verified.length, 1424);
for (const row of classSyntax.verified) {
  const roles = classMaps.get(row.index + ':' + row.map); assert(roles);
  assert.equal(row.codeSha256, row.producer === 'Native' ? roles.nativeJavaScriptSha256 : roles.actualJavaScriptSha256);
}
for (const [index, report] of privateReports.entries()) {
  assert.equal(report.family, 'ge3_decorator_compositions/' + privateForms[index]); assert.equal(report.completeFamily, true);
  completeReport(report, report.family, 7, report.verified.reduce((sum, row) => sum + row.controls.length, 0));
  assert.equal(report.executions.length, 7); assert(report.executions.every(row => row.executions === 2 && row.directNodePreservedSyntaxClaim === false));
}
assert.equal(privateReports.reduce((sum, report) => sum + report.verified.reduce((n, row) => n + row.controls.filter(control => control.rejected).length, 0), 0), 34201);
function executionTotals(report) {
  return { originalArtifactExecutions: report.verified.reduce((sum, row) => sum + row.runtime.executions, 0),
    additionalFixtureExecutions: report.verified.reduce((sum, row) => sum + row.runtime.additionalFixtureExecutions, 0) };
}
const objectExecution = executionTotals(objectRest), iterationExecution = executionTotals(iteration);
const arrowDefaultExecution = executionTotals(arrowDefault);
assert.deepEqual(arrowDefaultExecution, {originalArtifactExecutions: 54, additionalFixtureExecutions: 378});
assert.deepEqual(objectExecution, {originalArtifactExecutions: 782, additionalFixtureExecutions: 1564});
assert.deepEqual(iterationExecution, {originalArtifactExecutions: 758, additionalFixtureExecutions: 2274});
assert.equal(decoratorClassPhase.compilerSha256, compilerSha256); assert.deepEqual(decoratorClassPhase.failures, []);
assert.equal(decoratorClassPhase.rows.length, 64); assert(decoratorClassPhase.rows.every(row => row.repeatBuilds === 2));
assert.equal(decoratorClassPhase.rows.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 352);
const decoratorClassPhaseExecutions = decoratorClassPhase.rows.reduce((sum, row) => sum + Object.keys(row.native).length + Object.keys(row.blue).length, 0);
assert.equal(decoratorClassPhaseExecutions, 160);
const expressionSegmentAudit = { maps: 0, segments: 0, samePositionSegments: 0 };
for (const report of [namedAsync, nestedAsync, conditionalAsync, conditionalStatementAsync, templateAsync, templateGenerator]) {
  for (const row of report.verified) {
    const decoded = require('./ge5_maps.cjs').decode(reference.blobs[row.nativeMapSha256], reference.blobs[row.nativeJavaScriptSha256]);
    expressionSegmentAudit.maps++; expressionSegmentAudit.segments += decoded.segments.length;
    for (let index = 1; index < decoded.segments.length; index++) {
      const before = decoded.segments[index - 1], after = decoded.segments[index];
      if (before[0] === after[0] && before[1] === after[1]) expressionSegmentAudit.samePositionSegments++;
    }
  }
}
assert.deepEqual(expressionSegmentAudit, { maps: 42, segments: 2260, samePositionSegments: 0 });
assert(runtimeLog.includes('test exact_declarations_consumers_imports_syntax_and_execution_match_native ... ok'));
assert(!runtimeLog.includes('FAILED'));
assert(localLog.includes('test switch_local_binding_survives_a_later_generator_resume ... ok'));
assert(localLog.includes('test nested_binary_and_short_circuit_suspensions_preserve_native_resume_order ... ok'));
assert(localLog.includes('test async_joins_capture_receivers_and_propagate_rejections_like_native ... ok'));
assert(localLog.includes('test template_suspensions_match_native_getter_conversion_and_abrupt_completion_order ... ok'));
assert(!localLog.includes('FAILED'));
assert.equal(asynchronous.mutations.reduce((sum, row) => sum + row.controls.filter(control => control.rejected).length, 0), 18);
assert(roleLog.includes('test result: ok. 20 passed; 0 failed; 0 ignored;'));
assert(roleLog.includes('test result: ok. 5 passed; 0 failed; 0 ignored;'));
assert(!roleLog.includes('FAILED'));
for (const count of [1, 4]) assert(roleLog.includes(`test result: ok. ${count} passed; 0 failed; 0 ignored;`));
const roleResults = [...roleLog.matchAll(/test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;/g)]
  .reduce((sum, match) => ({targets: sum.targets + 1, passed: sum.passed + Number(match[1]),
    failed: sum.failed + Number(match[2]), ignored: sum.ignored + Number(match[3])}),
  {targets: 0, passed: 0, failed: 0, ignored: 0});
assert.deepEqual(roleResults, {targets: 15, passed: 41, failed: 0, ignored: 0});
assert(/\btests 31\b/.test(toolsLog) && /\bfail 0\b/.test(toolsLog));
const suspensionCases = new Set([...asynchronous.verified, ...generators.verified, ...continuations.verified, ...linear.verified, ...branching.verified, ...namedAsync.verified, ...nestedAsync.verified, ...conditionalAsync.verified, ...conditionalStatementAsync.verified, ...templateAsync.verified, ...templateGenerator.verified, ...computed.verified, ...switches.verified, ...finallyReturn.verified, ...catchFinally.verified, ...loopFinally.verified, ...delegatedFinally.verified].map(row => row.id));
assert.equal(suspensionCases.size, 405);
const bindingCases = new Set(bindingReports.flatMap(report => report.verified.map(row => row.id)));
assert.equal(bindingCases.size, 159);
const additionalCases = new Set([objectRest, iteration, classSuper, arrowDefault, lexicalArrow, objectMethod, classMethod, asyncGenerator, superMethod, g2Generator, legacyDecorator, ...privateReports].flatMap(report => report.verified.map(row => row.id)));
assert.equal(additionalCases.size, 2339);
const moduleCases = new Set([...optionalModules.verified, ...spreadModules.verified, ...taggedModules.verified].map(row => row.id));
assert.equal(moduleCases.size, 1089);
assert.equal(new Set([...suspensionCases, ...moduleCases, ...bindingCases, ...additionalCases]).size, 3992);
assert.equal(node.configurations, 146); assert.equal(node.javascriptMaps, 214); assert.equal(node.observations.length, 146);
assert.equal(native.configurations, 452); assert.equal(native.maps, 572); assert.equal(native.verified.length, 572);
const broadResults = [...broad.matchAll(/test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; (\d+) filtered out/g)]
  .reduce((sum, match) => ({ targets: sum.targets + 1, passed: sum.passed + Number(match[1]),
    failed: sum.failed + Number(match[2]), ignored: sum.ignored + Number(match[3]), filtered: sum.filtered + Number(match[4]) }),
  { targets: 0, passed: 0, failed: 0, ignored: 0, filtered: 0 });
assert.deepEqual(broadResults, { targets: 48, passed: 461, failed: 0, ignored: 73, filtered: 2 });
const covered = [], remaining = {}, forms = new Set(); let mapCount = 0;
for (const [index, specification] of input.cases.entries()) {
  const form = specification.form, expected = reference.cases[index]; assert.equal(expected.id, specification.id);
  const maps = Object.values(expected.maps).filter(map => map.kind === 'javascript').length;
  if (form.startsWith('gd2_declaration_maps/') || form.startsWith('regression/') || form.startsWith('ge2_module_helpers/node-') || suspensionCases.has(specification.id) || moduleCases.has(specification.id) || bindingCases.has(specification.id) || additionalCases.has(specification.id)) {
    forms.add(form); mapCount += maps;
    covered.push({ id: specification.id, contract: moduleCases.has(specification.id) ? 'complete original/lowered expression, module ABI and generated-gap profile'
      : additionalCases.has(specification.id) ? 'complete object/iteration/class/private AST, module ABI and generated-gap profile'
      : bindingCases.has(specification.id) ? 'complete binding evaluation, helper/continuation ABI and generated-gap profile'
      : suspensionCases.has(specification.id) ? 'complete async/native-generator AST and generated-gap profile'
      : form.startsWith('ge2_module_helpers/node-') ? 'Node bridge' : 'retained syntax',
      javascriptMaps: maps, repeatBuilds: 2, accepts: expected.diagnostics.length === 0 });
  } else {
    const suite = specification.origin.suite;
    remaining[suite] ||= { forms: new Set(), configurations: 0, javascriptMaps: 0 };
    remaining[suite].forms.add(form); remaining[suite].configurations++; remaining[suite].javascriptMaps += maps;
  }
}
assert.equal(covered.length, 4755); assert.equal(forms.size, 95); assert.equal(mapCount, 4688);
const coveredIds = new Set(covered.map(row => row.id)), familyTotals = new Map();
for (const row of input.cases) {
  const count = familyTotals.get(row.form) || { total: 0, covered: 0 };
  count.total++; count.covered += Number(coveredIds.has(row.id)); familyTotals.set(row.form, count);
}
const fullyCoveredFamilies = [...familyTotals.values()].filter(count => count.covered === count.total).length;
const partiallyCoveredFamilies = [...familyTotals.values()].filter(count => count.covered > 0 && count.covered < count.total).length;
assert.equal(fullyCoveredFamilies, 95); assert.equal(partiallyCoveredFamilies, 0);
for (const group of Object.values(remaining)) group.forms = [...group.forms].sort();
assert.equal(Object.values(remaining).reduce((sum, group) => sum + group.configurations, 0), 3652);
assert.equal(Object.values(remaining).reduce((sum, group) => sum + group.javascriptMaps, 0), 5505);
assert.equal(Object.values(remaining).reduce((sum, group) => sum + group.forms.length, 0), 42);
assert(fs.readFileSync(path.join(phase, 'TODO.md'), 'utf8').includes('- [ ] **G-E5'));
const sourceFiles = {};
function sources(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) sources(file);
    else if (/\.(rs|js)$/.test(file)) sourceFiles[path.relative(root, file)] = sha256(fs.readFileSync(file));
  }
}
sources(path.join(root, 'backend/bluets/src')); sources(path.join(root, 'backend/bluets-bluejs/src'));
sources(path.join(root, 'backend/bluets/tests/oracle_support'));
for (const file of [...fs.readdirSync(path.join(root, 'backend/bluets/tests')).filter(name => /^ge5_.*\.rs$/.test(name)).map(name => 'backend/bluets/tests/' + name),
  'backend/bluets/tests/ge1_downlevel.rs', 'backend/bluets/tests/gc1_explain_files.rs',
  'backend/bluets/tests/project_build_corrupt_state.rs', 'backend/bluets/tests/project_build_watch.rs',
  'backend/bluets/tests/project_referenced_declarations.rs',
  'backend/bluets-bluejs/tests/emitted_target_editions.rs',
  'backend/bluets-bluejs/tests/template_suspension_order.rs',
  'development/browser_core/phase-18-bluets/tools/record_ge1_downlevel.cjs',
  'development/browser_core/phase-18-bluets/tools/ge1_observations.cjs',
  'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs',
  'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs',
  'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs',
  'backend/bluets/tests/oracle_support/ge5_node_roles.rs', 'backend/bluets/tests/oracle_support/ge5_javascript_roles.rs',
  'backend/bluets/tests/oracle_support/ge5_javascript_maps.rs', 'backend/bluets/tests/oracle_support/ge5_async_roles.rs',
  'backend/bluets/tests/oracle_support/ge5_javascript_boundaries.rs',
  ...fs.readdirSync(__dirname).filter(name => /^(?:ge5_|verify_ge5_|report_ge5_).*\.cjs$/.test(name)).map(name => path.relative(root, path.join(__dirname, name)))]) {
  sourceFiles[file] = sha256(fs.readFileSync(path.join(root, file)));
}
const measurement = { schema: 2, state: 'partial; G-E5 open; full parity OFF',
  cacheIdentity: { javascriptProvenance: 20, privateStateEpoch: 45, rebuiltEpochs: Array.from({ length: 41 }, (_, index) => index + 4) },
  compilerSha256, inputSha256: sha256(inputBytes), referenceSha256: sha256(referenceBytes),
  covered: { configurations: 4755, fullyCoveredSourceFamilies: fullyCoveredFamilies, partiallyCoveredSourceFamilies: partiallyCoveredFamilies,
    sourceFamiliesWithEvidence: 95, javascriptMaps: 4688, repeatBuilds: 2, cases: covered },
  remaining: { configurations: 3652, sourceFamilies: 42, javascriptMaps: 5505, groups: remaining },
  node, asynchronous, generators, continuations, linear, branching, namedAsync, nestedAsync, conditionalAsync, conditionalStatementAsync, templateAsync, templateGenerator,
  optionalModules, moduleExecution, spreadModules, spreadExecution, taggedModules, taggedExecution, computed, computedExecution, switches, switchExecution, finallyReturn, catchFinally, exceptionExecution, loopFinally, loopExecution, delegatedFinally, delegateExecution, bindingReports, bindingExecution, decoratorPublication, objectRest, objectExecution, iteration, iterationExecution, classSuper, classExecution, classSyntax, arrowDefault, arrowDefaultExecution, lexicalArrow, objectMethod, classMethod, asyncGenerator, superMethod, g2Generator, legacyDecorator, privateReports, decoratorClassPhase, decoratorClassPhaseExecutions, behaviorEvidenceReuse, expressionSegmentAudit, nativeCoordinateReplay: native,
  gates: { broadDevelopment: broadResults, debuggerGenerated: { passed: 22, failed: 0, ignored: 0, filtered: 33 },
    clippy: 'both BlueTS crates; all targets; warnings denied', formatExitStatus: format.status,
    finalImmutableCohort: 'pending', finalUnfilteredOrdinary: 'pending',
    broadLogSha256: sha256(broad), debuggerLogSha256: sha256(debuggerLog), clippyLogSha256: sha256(clippy),
    focusedBoundaryLogSha256: sha256(boundaryLog), runtimeLogSha256: sha256(runtimeLog), localLogSha256: sha256(localLog),
    roleLogSha256: sha256(roleLog), roleOracles: { passed: 41, configurations: 3992, repeatBuilds: 2, rejectedControls: 175362 },
    toolsLogSha256: sha256(toolsLog), mapAndNativePatchTools: { passed: 31, failed: 0 },
    ge1Runtime: { configurations: 500, consumerChecks: 1000, executions: 500 } },
  sourceFiles: Object.fromEntries(Object.entries(sourceFiles).sort(([a], [b]) => a.localeCompare(b))) };
const document = [
  '# G-E5 source-role implementation progress', '',
  '**Status: incomplete.** G-E5 remains unchecked and full parity stays OFF. The 572 Native coordinate corrections resolve one root cause; they do not establish JavaScript lowering-map parity.', '',
  "The complete role/gap contract passes **4,755 configurations / 4,688 JavaScript maps**, with two public CLI builds per configuration. Exactly **95 families are fully covered**: 43 G-D2/signature/enum families, eight Node families, three whole-module helper families, ten generator families, five named async families, five binding families, seven class/super families, four preserved private/decorator families, two object/iteration families, two async-arrow families, one async-object-method family, one async-class-method family, one async-generator exception/template family, one async-super-method family, one whole-module async-generator/finally family and one legacy-parameter-decorator family. The ten generator and five named async families, default-arrow family and object-method family each pass all 27 frozen products; the lexical-receiver arrow family passes all 51 target/field/publication products. No family is counted as partially complete. Declaration-only and rejected products remain in the configuration count.", '',
  'The contract checks every token and its UTF-16 interior owners, complete map metadata, ordered gap owners, and all generated-only code. Only structurally identified, fresh compiler bindings may be renamed: reviewed Node/module transport bindings, optional/nullish capture slots and the continuation context. Source bindings retain their names. Formatting can remove a gap, add an unmapped gap or reset a physical line; each surviving mapped gap position must follow Native ownership in order. Identical JavaScript requires every decoded map entry to match. Mutation controls reject missing tokens, retargeted interiors and source grants in generated comments. This is not an anchor inventory or a BlueTSC output snapshot.', '',
  'Non-ES5 async profiles parse both complete emitted modules using pinned TypeScript 5.9.3. Named async signatures and bodies are matched through the explicit driver ABI, every UTF-16 code unit is classified, generated helper/guard/grouping ranges must remain unmapped, and the relocated callback close and driver completion retain Native owners. Eighteen isolated, bounded mutations across original and CRLF/astral sources are rejected, including token interiors, helpers, guards, grouping, body ends, driver completion, gaps and trailing comments. Observations are bound to the actual compiler binary and complete frozen selection.', '',
  'The complete opaque-generator continuation contract follows every reachable state and rejects extra/unreachable states, a missing resume, wrong local slots or unreviewed completion shapes. Retained switch syntax, yield operands, opcode boundaries, resumed values and terminators use Native owners; dispatch and helper definitions remain unmapped. Its seven publication products reject 70 isolated mapping/control-flow mutations. All 500 G-E1 programs also pass their 1,000 declaration consumers and 500 actual executions.', '',
  'Binary and short-circuit continuations use independent Native/BlueTSC data-flow traces. The entire expression DAG, original grouping, operator/token owners, carried values, user writes and completion endpoints are compared. All four abstract short-circuit paths are checked, including branches that the fixed runtime input does not take; every reviewed state is covered. Unmatched expression evaluations, wrong operations/slots/bindings, inverted branches and skipped resumes fail. These two complete seven-product sets reject 175 additional bounded mapping/control-flow mutations. The four ES5 async sets reject another 399 controls, including the wrong captured receiver, changed methods, inverted branches, wrong user writes and erased carried returns; Template await/yield add 287 controls and computed assignments add 238 and switch fallthrough adds 301; finally return and catch/finally add 728 more, for 2,216 controls in total. Native and BlueTSC also execute the same multi-resume arithmetic/short-circuit regression. Async execution checks both expression/statement conditional arms, helper collisions, receiver/getter capture order and rejection propagation. Template execution also checks captured concat getters, argument grouping, string-hint conversion and early return/throw.', '',
  'The two template contracts compare both complete captured concat calls and their full data-flow, all interpolation and logical template endpoints, and the discarded generator resume. Logical template groups retain the Native whole-expression boundary; generated literal prefetch and helper transport remain unmapped. A deleted opening must fail against Native authority even when the remaining grouping is syntactically valid. Their 287 mutations include that deletion, wrong fixed text, changed methods/receivers and skipped or replaced resumes.', '',
  'Computed assignments compare complete dense-array inventories, object/key reference identity, ordered binding reads across each suspension, carried RHS values and write/statement endpoints. All seven ES5 publication products reject 238 bounded map/code mutations, including a delayed re-read of the array binding. Native and BlueTSC pass 14 original artifact executions and 42 supplemental executions with a zero key, rebinding during suspension and early return before the write. The rebinding fixture verifies both the saved original array and the new binding. Intact initializer statements retain their original ending; split-initializer stores retain the expression ending. The existing logical-yield products pass the same integration run.', '',
  'Switch fallthrough follows all three complete selection paths through Native and BlueTSC continuations. Synchronous case tests retain switch selection, each source value/colon/jump, ordered fallthrough, resumed user writes and source-anchored break endpoints. Tests with suspension retain the existing continuation chain; the external helper ABI is unchanged. All seven ES5 publication products reject 301 bounded map/code mutations. Native and BlueTSC pass 14 original artifact executions and 70 supplemental executions with direct case selection, default, a zero resume value, early return and external throw.', '',
  'The finally-return and catch/finally contracts classify all exception registrations, try/catch/finally phases, pending and overriding returns, source throws, hygienic catch aliases and effects, normal leaves, end-finally routing and the exact generated dead-state inventory. All fourteen ES5 publication products reject 728 bounded map/code mutations. Native and BlueTSC pass 28 original artifact executions and 238 supplemental executions across abrupt completions before start, in the try body, in catch, in finally and after completion, plus an otherwise unselected default-return path. Supplemental fixtures suppress the original step initializer so each tested iterator starts fresh. No production correction is required for these two families.', '',
  'Loop/finally compares bounded independent scalar state, every iteration and hygienic lexical identity derived from the original declaration coordinate. Normal leave, continue and break retain distinct region destinations. Source if prefixes/closing boundaries and the latent guard exit use Native roles, including a static check that the unselected exit reaches the final continuation. For-header initializer endings remain distinct from standalone declarations. All seven ES5 publication products reject 364 bounded map/code mutations; Native and BlueTSC pass 14 original and 126 supplemental executions across empty loops, continue/guard exits, suppressed continue/break conditions and return/throw during body/finally suspension.', '',
  'Implemented corrections include fused template chunks, complete interpolation/template endpoints and logical carried-template origins, moved ES5 async signatures, retained method receivers/properties, separated captured-resume endpoints, joined conditional returns, compound write/read/operator origins and distinct statement terminators, retained async/template operands, relocated body/completion endpoints, non-suspending switch syntax, ES5 expression grouping/operator/carried-value origins and complete declaration initializer endpoints. Optional/nullish lowering retains captured receiver, property, predicate, fallback and grouping owners across all module ABIs; retained getter/object and original grouping endpoints also follow Native. System variable removal excludes the deleted keyword trivia. Empty parameter/call parentheses retain their opening owner through the following gap, including a returned empty-parameter arrow. Array/call spread preserves copied operands, inherited helper prefixes and separate callee/argument/closing boundaries. System hoisting carries original comments and function endpoints; its lowered array-assignment terminators keep their distinct Native owners. Tagged templates also retain every raw-head/tail and substitution owner, plus distinct declaration-initializer versus assignment completion endpoints. Computed writes preserve their complete saved reference and operation owner; dense-array initializer statements also retain their distinct ending segment. JavaScript provenance identity is v20 and build-state epoch is 45; epochs 4–44 rebuild.', '',
  'The complete optional/nullish family covers all 363 configurations and JavaScript maps, including every module, target, helper flag and publication product. Independent module ABI contracts validate CommonJS, ESM, AMD, both UMD branches and System registration/publication, together with complete expression roles and generated-only helpers/transport. All 8,515 isolated map/code mutations are rejected, bringing the suspension plus optional/nullish subtotal to 9,464. Actual execution covers 806 original Native/BlueTSC artifact runs; 1,612 additional null/undefined runtime-fixture runs exercise both strict nullish paths. Those supplemental runtime fixtures do not replace or enlarge the frozen map acceptance cohort.', '',
  'The complete array/call-spread family adds all 363 frozen configurations and maps, including imported or global tslib helpers, module setters/callbacks, complete coherent inline helper groups and the generated invocation ABI. Original leaves and every physical transport character follow independently derived Native roles. All 9,397 isolated bounded mutations fail, bringing the suspension plus optional/nullish/spread subtotal to 18,861; mutations include disabled sparse packing, unexpected iteration, changed intrinsics/dependencies and source grants in helpers/imports. Native and BlueTSC pass 806 original artifact executions and 806 separate sparse runtime-fixture executions. The latter preserve all three copied indexes, including an explicit undefined element at the original hole. Frozen map acceptance remains unchanged.', '',
  'The complete tagged-template family adds every one of its 363 frozen configurations and maps. All site-cache references and assignments, the complete constructor/import ABI, cooked/raw contents, raw span boundaries, substitution interiors and generated transport are checked. Assignment calls end at the final substitution; retained declaration initializers keep the complete template extent. Their 8,533 bounded mutations fail, bringing the suspension plus module-family subtotal to 27,394. All 806 original artifact executions and 806 separate repeated-site fixture executions pass, checking stable site identity, cooked/raw strings, raw descriptors and target-specific Native freezing behavior. An ordinary public-compiler boundary regression compares raw/substitution/statement endpoints for CommonJS/ESM and original/CRLF/astral sources. Helper definitions remain unmapped.', '',
  'Computed assignment/yield completes its remaining seven ES5/publication products, so the family now passes all 27 frozen products. The independent continuation contract includes dense array construction, element reads, saved object/key references and ordered writes, with original binding reads checked against their yield/resume phases. A late user-binding read, wrong saved key/RHS, skipped resume or erased reference/operation/endpoint fails. All 238 new bounded controls are rejected, bringing the total to 27,632. Fourteen original artifact executions and 42 separate key-zero, rebound-array and early-return fixture executions pass; rebinding the user variable while suspended still writes the saved array. The ordinary compiler boundary regression verifies complete reference interiors, the operation and initializer statement endpoint for both coordinate variants.', '',
  'Delegation/finally now closes its seven ES5 products. Both complete child/outer continuation graphs preserve pending returns through their finalizers, and the delegated call, sent result, helper dependency identities, all UTF-16 owners and generated gaps are checked. Its 546 map/code/helper controls fail; 14 original and 126 supplemental executions cover normal completion, return/throw in each finalizer, before start and after completion.', '',
  'The five complete binding families add 159 frozen configurations/maps and 13,812 rejected controls. Root initialization, nested leaves, defaults, parameter prologues, getters and iterator-finally lifecycles retain Native evaluation order and complete token/gap roles. All 318 original and 798 supplemental executions pass. Source-map fixes also separate target-specific named class publication, System assignment/completion and unmapped publication transport. A separate 32-configuration decorator cycle oracle passes 80 actual loader executions and 56 deletion controls; these supplemental rows do not enlarge the frozen map cohort or close any decorator family.', '',
  'Object/rest-spread and iterable-binding/for-of close all 690 frozen configurations/maps. Their complete module, helper and continuation contracts reject 44,376 bounded controls; Native and BlueTSC pass 1,540 original executions and 3,838 supplemental getter/symbol/descriptor/abrupt-completion and iterator-finalizer executions. System moved-function completion stays in the factory; execute trivia is unmapped while adjacent real statement starts retain their own owners.', '',
  'All seven complete class/super families cover 661 configurations / 712 maps, reject 8,269 controls, pass 1,402 artifact executions and parse all 1,424 Native/Blue programs at the selected edition. The prior 43-configuration super-write subset is included once in coverage and controls. Four preserved private/decorator families close all 28 configurations/maps with 34,201 controls, including coherent inline-map mutations. Their 56 emitted-JavaScript-only adapter executions retain the exact frozen Native result, including the expected private-super SyntaxError; no direct Node preserved-decorator syntax support is claimed.', '',
  'Public boundary regressions also preserve original leading comment interiors, completion and the following ordered gap; retained new-callee grouping; named-import delimiters and from-clause completion; comments attached to erased declarations remain unmapped; async-arrow captures/defaults/signatures and relocated class-field subtrees. A supplementary 64-configuration legacy class-phase oracle rejects 352 controls and passes 160 executions (both UMD branches included), preserving computed keys before the class factory and static/member/class initialization inside it. These supplementary and boundary checks do not count an additional frozen family.', '',
  'The complete async-arrow/default family adds all 27 frozen products, 745 controls and 432 executions (54 original plus 378 default/provided/nullish/string/rejection probes). The ES5 default guard has a closed original-AST/fully verified UTF-16 prologue capability and complete three-state routing; the core continuation remains independently checked. Callback headers, first-body gaps and callback/driver logical endings retain distinct Native phases. The complete legacy parameter-decorator family adds all 363 products, 6,805 coherent controls and 986 actual loader scenarios, including exact metadata/helper/class/module/publication roles.', '',
  'The complete lexical-receiver arrow family adds all 51 products, 1,916 controls and 1,326 executions (102 original plus 1,224 supplemental). A source-bound constructor/factory/callback receiver chain, complete field initializer proof and exact generated result grouping retain every source role without alias allowlists. The complete object-method family adds all 27 products, 2,469 coherent controls and 756 executions (54 original plus 702 supplemental). Dynamic this and arguments flow through the apply/awaiter/generator chain; generated transport remains unmapped and method body/driver endpoints retain distinct Native owners. Borrowed, bound, detached, getter-order, mutation-before-await and rejection scenarios are executed in real CommonJS and ESM modules.', '',
  'The complete async-class-method family adds all 51 products, 5,419 coherent controls and 1,530 executions (102 original plus 1,428 supplemental). Typed original method signatures and endpoints survive ES5 descriptor/callback relocation and generated apply transport. A fully proved method subtree composes with the strict enclosing class, field and module contract; the parent retains descriptor and surrounding gap authority. Dynamic receivers, borrowed/bound/detached calls, arity, getter/evaluation order and rejection paths pass in real CommonJS and ESM modules. The complete async-generator exception/template family passes all 27 products, 1,466 controls and 1,026 executions (54 original plus 972 supplemental, including borrowed iterator methods). Both physical state machines, pending try/finally completions and await/yield/return phases are proved before comparing the same logical events. Concurrent next/return/throw requests, promised returns and abrupt getters retain the pinned Native target behavior. Both producers pass target-specific Native/Blue syntax checks. Its original neighboring runtime-only evidence has since been superseded by the complete 363-product contract below.', '',
  'The complete async-super-method family adds all 51 frozen products, 6,741 coherent controls and 1,428 executions (102 original plus 1,326 supplemental). Exact original-class/receiver/bridge/member proofs cover every physical character before a closed WeakMap capability delegates the super lookup to the continuation DAG. Base getter and invocation reads stay after the await; a forged view, callback, source or binding cannot grant this role. Empty ES5 signatures, named property tokens, generated invocation closes and subsequent arithmetic operators retain Native phases. Real borrowed/bound/detached calls, late base replacement, getter/call ordering and rejection paths agree with Native. The complete whole-module async-generator/finally family adds 363 products, 19,572 coherent controls and 12,896 executions (806 original plus 12,090 supplemental, both UMD branches included). Its exact private/ambient/import dependencies, complete pending-return/finally state machines, export/header phases, operand endpoints, comments and logical endings are proved. Private lowered iterator methods use Native closure receiver behavior; retained intrinsic methods preserve their native receiver checks. G1 generator replay now includes borrowed next/return/throw, adding 162 executions without changing its 27 frozen products or 1,466 map controls. The shared runtime loader only publishes ambient helpers for noEmitHelpers without importHelpers, so private artifacts execute their actual emitted algorithms; twelve independent routing controls and sixty captured receiver/argument profiles pass.', '',
  '## Remaining implementation and contracts', '',
  '| Suite | Source families | Configurations | JavaScript maps |', '| --- | ---: | ---: | ---: |',
  ...Object.entries(remaining).map(([suite, group]) => `| ${suite} | ${group.forms.length} | ${group.configurations} | ${group.javascriptMaps} |`),
  '| **Total** | **42** | **3,652** | **5,505** |', '',
  'These admitted configurations remain required. Their complete source-role/generated-gap contracts and associated lowering provenance must be implemented; bounds and metadata alone cannot close them. The remaining work is compiler implementation plus contract coverage, followed by the complete immutable cohort and unfiltered ordinary gate.', '',
  '## Current verification', '',
  '- Development regression: 48 targets / 461 passed / zero failed / 73 ignored oracle functions / two full G-E5 functions filtered. All 41 role oracle functions were executed separately: 3,992 configurations, two builds each, and 175,362 rejected bounded mutations. This is not the final ordinary gate.',
  '- Seven emitted-target/runtime tests pass, including concat getter/argument grouping/string-hint conversion and generator early return/throw. The ordinary template boundary regression includes each logical interpolation endpoint.',
  '- Current compiler replays all 452 Native-defect configurations / 572 maps twice; bounds and generated execute braces pass.',
  '- The two ES5 resume/template regressions pass for original and CRLF/astral sources. G-E1 owner, atomicity, parser/work budgets, helper/caches and iterator/finally controls pass, including rebuilds from old state. These targeted ES5 checks do not establish the pending whole-module ES5 contracts.',
  '- K.11 actual launcher/core/child generated matrix: 22 passed / zero failed or ignored / 33 unrelated functions filtered; unchanged eleven-row helper matrix and stale/substituted/malformed negatives pass.',
  behaviorEvidenceReuse ? '- Same-binary runtime/process evidence retains its original bound log hashes.' : '- All behavior gates are refreshed for provenance v20: seven local tests, 500 programs / 1,000 declaration consumers / 500 executions, and 22 K.11 real-process tests. No older production evidence is substituted.',
  '- Both BlueTS crates pass all-target warnings-denied Clippy; workspace format check passes.',
  '- Frozen G-E5 inputs/reference and prior fixture bytes remain unchanged. No G-E5 completion, commit or push is claimed.', '',
  '[Machine-readable measurements and remaining family list](GE5_ROLE_CONTRACTS_MEASUREMENT.json).', '',
].join('\n');
for (const [name, text] of Object.entries({ 'GE5_ROLE_CONTRACTS.md': document,
  'GE5_ROLE_CONTRACTS_MEASUREMENT.json': JSON.stringify(measurement, null, 2) + '\n' })) {
  const file = path.join(phase, name);
  if (process.argv.includes('--write')) fs.writeFileSync(file, text);
  else assert.equal(fs.readFileSync(file, 'utf8'), text, name + ': measurement changed');
}
console.log(JSON.stringify({ coveredConfigurations: 4755, coveredJavascriptMaps: 4688,
  fullyCoveredFamilies: 95, partiallyCoveredFamilies: 0,
  remainingConfigurations: 3652, remainingJavascriptMaps: 5505, compilerSha256 }));
