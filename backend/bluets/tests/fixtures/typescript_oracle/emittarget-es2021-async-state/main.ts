// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export async function calculate(): Promise<number> {
  const left = await Promise.resolve(20);
  const right = await Promise.resolve(22);
  return left + right;
}
calculate().then(value => console.log(value));
