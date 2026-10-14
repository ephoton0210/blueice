// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let closed = 0;
export async function* sequence(): AsyncGenerator<number, string, unknown> {
    try {
        yield await Promise.resolve(20);
        yield 22;
        return "done";
    } finally { closed++; }
}
export async function* recovery(): AsyncGenerator<number, string, unknown> {
    try { yield 1; }
    catch (error) { yield 42; }
    return "done";
}
export function count(): number { return closed; }
