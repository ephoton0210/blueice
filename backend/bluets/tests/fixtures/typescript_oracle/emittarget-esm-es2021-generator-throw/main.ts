// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export function* sequence(): Generator<number, number, unknown> {
  try { yield 20; }
  catch (error) { if (error === 42) { yield error; } else { throw error; } }
  finally { console.log("closed"); }
  return 0;
}
const iterator = sequence();
console.log(iterator.next().value);
console.log(iterator.throw(42).value);
console.log(iterator.next().value);
