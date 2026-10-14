// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
const counter = {
  base: 10,
  add(n: number): number { return n + 1; },
  label(prefix: string, suffix: string = "!"): string { return prefix + suffix; },
  get answer(): number { return 42; },
  compose(f: (n: number) => number, n: number): number { return f(f(n)); },
};
console.log(counter.add(1));
console.log(counter.label("a"));
console.log(counter.answer);
console.log(counter.compose(counter.add, 5));
