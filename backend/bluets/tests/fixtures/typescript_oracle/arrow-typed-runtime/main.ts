// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
const double = (n: number): number => n * 2;
const greet = (name: string, punct: string = "!"): string => { return "hi " + name + punct; };
const twice = (f: (n: number) => number, n: number): number => f(f(n));
const add = (a: number) => (b: number): number => a + b;
const label = (n?: number): string => { if (n) { return "n"; } return "none"; };
const pair = (): [number, string] => [1, "a"];
console.log(double(4));
console.log(greet("a"));
console.log(greet("a", "?"));
console.log(twice(double, 3));
console.log(add(1)(2));
console.log(label());
console.log(pair()[1]);
