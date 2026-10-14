// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let reads = 0;
const first = { get value(): number { reads++; return 20; } };
const result = { ...first, value: 42 };
export const answer = result.value;
console.log(answer, reads);
