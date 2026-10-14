// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export function* sequence(): Generator<number, number, unknown> {
  try { yield 20; yield 22; return 0; }
  finally { console.log("finally"); yield 1; }
}
const iterator = sequence();
console.log(iterator.next().value);
const pending = iterator.return(42);
console.log(pending.value, pending.done);
const completed = iterator.next();
console.log(completed.value, completed.done);
