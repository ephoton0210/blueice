// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Real native watchers: mutate admitted files only after the previous cycle ends.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const native = require('./gm2_native.cjs');
const observe = require('./gm2_observations.cjs');

async function run(ts, workflow, snapshot, emit = () => {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-gm2-watch-')));
  const inputs = {...workflow.files};
  observe.write(root, inputs);
  const diagnostics = [], events = [];
  let waiting, timer, builder;
  function next() {
    return new Promise((resolve, reject) => {
      waiting = resolve;
      timer = setTimeout(() => reject(Error(workflow.id + ': native watch timeout')), 20000);
    });
  }
  const host = native.host(ts, root, diagnostics, events, true, message => {
    if (!ts.flattenDiagnosticMessageText(message.messageText, '\n').includes('Watching for file changes')) return;
    clearTimeout(timer);
    const complete = waiting;
    waiting = null;
    if (complete) complete();
  });
  const result = [];
  try {
    let completed = next();
    builder = ts.createSolutionBuilderWithWatch(host, [path.join(root, 'tsconfig.json')], {verbose: true}, {
      watchFile: ts.WatchFileKind.FixedPollingInterval,
      watchDirectory: ts.WatchDirectoryKind.FixedPollingInterval,
      synchronousWatchDirectory: true,
    });
    builder.build();
    await completed;
    for (const [index, step] of workflow.steps.entries()) {
      if (index) {
        // Let newly admitted file watchers finish registration before editing.
        await new Promise(resolve => setTimeout(resolve, 25));
        completed = next();
        for (const changes of [step.changes, ...(step.burst || [])]) {
          observe.changes(root, changes);
          for (const [name, text] of Object.entries(changes)) {
            if (text === null) delete inputs[name]; else inputs[name] = text;
          }
        }
        await completed;
      }
      const row = {id: step.id, step: step.step, ...snapshot(root, workflow, inputs,
        {exit: diagnostics.length ? 1 : 0, events: events.splice(0), diagnostics: diagnostics.splice(0)})};
      const clean = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-gm2-clean-')));
      try {
        observe.write(clean, inputs);
        const fresh = native.build(ts, clean, false);
        assert.equal(fresh.exit, 0, workflow.id + ': native clean build');
        const outputs = observe.collect(clean, inputs);
        assert.deepEqual(row.outputs, outputs, step.id + ': native watch/clean bytes');
        row.cleanOutputs = outputs;
      } finally { fs.rmSync(clean, {recursive: true, force: true}); }
      result.push(row);
      emit(row);
    }
    return result;
  } finally {
    clearTimeout(timer);
    if (builder) builder.close();
    fs.rmSync(root, {recursive: true, force: true});
  }
}
module.exports = {run};
