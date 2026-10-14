// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function* count(n: number): Generator<number, number, boolean> {
    let i = 0;
    while (i < n) {
        const stop: boolean = yield i;
        if (stop) { return -1; }
        i++;
    }
    return n;
}
function* plain(): Generator<number> { yield 1; yield 2; }
function* delegating(): Generator<number, string, undefined> {
    yield* plain();
    yield* [10, 20];
    const inner: string = yield* nested();
    return inner;
}
function* nested(): Generator<number, string> { yield 99; return "inner"; }
const make = function* (n: number): Generator<number> { yield n; yield n * 2; };
const holder = { *items(): Generator<number> { yield 7; yield 8; } };
const g = count(3);
console.log(g.next().value, g.next(false).value, g.next(true).value, g.next().done);
const d = delegating();
const seen: number[] = [];
let step = d.next();
while (!step.done) { seen.push(step.value); step = d.next(); }
console.log(seen.length, seen[0], seen[4], step.value);
let total = 0;
for (const v of make(5)) { total += v; }
for (const v of holder.items()) { total += v; }
console.log(total, [...plain()].length);
