// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export async function calculate(): Promise<number> {
  try { await Promise.reject(20); return 0; }
  catch (error) { return error === 20 ? 42 : 0; }
  finally { console.log("closed"); }
}
calculate().then(value => console.log(value));
