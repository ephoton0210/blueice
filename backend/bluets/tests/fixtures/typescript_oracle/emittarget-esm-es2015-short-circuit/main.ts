// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let calls = 0;
function receiver(): { value: number } | undefined { calls++; return { value: 42 }; }
export const result = receiver()?.value ?? 0;
console.log(result, calls);
