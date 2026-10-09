// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export async function calculate(): Promise<number> {
  try { return await Promise.resolve(20); }
  finally { await Promise.resolve(0); return 42; }
}
calculate().then(value => console.log(value));
