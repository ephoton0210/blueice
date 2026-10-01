// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function* count(n: number): Generator<number, number, boolean> {
    for (let i = 0; i < n; i++) {
        const stop: boolean = yield i;
        if (stop) { return -1; }
    }
    return n;
}
function* plain(): Generator<number> { yield 1; yield 2; }
function* delegating(): Generator<number, void, undefined> { yield* plain(); yield* [10, 20]; }
function* inferred() { yield 1; yield 2; return 3; }
function* idle(): Generator<number> {}
const make = function* (): Generator<string> { yield "a"; };
const holder = { *items(): Generator<number> { yield 7; } };
export const a: number = [...plain()].length;
export const b: number = count(0).next().value;
let total = 0;
for (const v of delegating()) { total += v; }
export const c: number = total + holder.items().next().value;
export const d: number = [...inferred()].length + [...make()].length + [...idle()].length;
