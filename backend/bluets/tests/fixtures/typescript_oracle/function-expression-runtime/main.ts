// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
const double = function (n: number): number { return n * 2; };
const greet = function named(name: string, punct: string = "!"): string { return "hi " + name + punct; };
const twice = function (f: (n: number) => number, n: number): number { return f(f(n)); };
const xs = [1, 2, 3].map(function (n: number): number { return n + 1; });
console.log(double(4));
console.log(greet("a"));
console.log(twice(double, 3));
console.log(xs[2]);
