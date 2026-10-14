// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export async function* sequence(): AsyncGenerator<number, void, unknown> {
  yield await Promise.resolve(42);
}
async function run(): Promise<void> {
  for await (const value of sequence()) { console.log(value); }
}
void run();
