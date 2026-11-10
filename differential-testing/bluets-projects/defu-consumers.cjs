// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

module.exports = {
  positive: `import defu, {createDefu} from './dist/defu';
import type {Defu} from './dist/defu';
import type {DefuFn, Merger} from './dist/types';
import {isPlainObject} from './dist/_utils';
const result = defu({port: 3000, nested: {enabled: true}}, {host: 'local', nested: {retries: 2}});
const port: number = result.port;
const host: string = result.host;
const enabled: boolean = result.nested.enabled;
const retries: number = result.nested.retries;
const merger: Merger = (object, key, value, namespace) => false;
const custom: DefuFn = createDefu(merger);
const flag: boolean = isPlainObject(result);
const shape: Defu<{port: number}, [{host: string}]> = {port: 1, host: 'local'};
`,
  negative: `import defu from './dist/defu';
const result = defu({port: 3000}, {host: 'local'});
const wrong: boolean = result.port;
`,
  typeBoundary: `import defu from './dist/defu';
import type {DefuFn} from './dist/types';
const callable: DefuFn = defu.fn;
`,
};
