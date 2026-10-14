// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { isString, assertString } from './helper.ts'; export function read(value: unknown): string { if (isString(value)) { return value; } assertString('fallback'); return 'fallback'; } console.log(read('ok'), read(42));
