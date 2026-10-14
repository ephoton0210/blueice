// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let calls = 0;
const source = {
  get left(): number | undefined { calls++; return undefined; },
  right: 22
};
function fallback(): number { calls++; return 20; }
const { left = fallback(), right, ...rest } = source;
export const result = left + right;
console.log(result, calls, "left" in rest);
