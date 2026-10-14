// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let closed = 0;
function* inner(): Generator<number, void, unknown> {
  try { yield 20; yield 22; } finally { closed++; }
}
export function* outer(): Generator<number, number, unknown> {
  yield* inner();
  return 0;
}
const iterator = outer();
console.log(iterator.next().value);
console.log(iterator.return(42).value, closed);
