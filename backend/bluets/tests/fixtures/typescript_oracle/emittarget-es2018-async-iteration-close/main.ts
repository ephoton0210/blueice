// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let closed = 0;
export async function* sequence(): AsyncGenerator<number, void, unknown> {
  try { yield await Promise.resolve(42); yield 0; }
  finally { closed++; }
}
async function run(): Promise<void> {
  for await (const value of sequence()) { console.log(value); break; }
  console.log(closed);
}
void run();
