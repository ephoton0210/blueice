// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let left = 0;
let right: number | undefined;
left ||= 20;
right ??= 22;
let enabled = 42;
enabled &&= left + right;
export const result = enabled;
console.log(result);
