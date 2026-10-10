// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
const source = { left: 20, right: 22, extra: 1 };
const { left, right, ...rest } = source;
const [first, ...tail] = [left, right];
export const result = first + tail[0] + rest.extra - 1;
console.log(result);
