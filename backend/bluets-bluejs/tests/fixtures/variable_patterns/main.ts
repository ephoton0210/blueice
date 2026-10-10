// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let calls = 0;
const source = {
    get left(): number | undefined { calls++; return undefined; },
    right: 22
};
function fallback(): number { calls++; return 20; }
const {left = fallback(), right, ...rest} = source;
const [first, ...tail] = [left, right];
export const answer = first + tail[0];
export const reads = calls;
export const restOwn = "left" in rest;
