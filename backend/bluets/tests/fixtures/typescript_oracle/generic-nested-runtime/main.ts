// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
const identity = <T>(value: T): T => value;
const pair = function <A, B>(a: A, b: B): [A, B] { return [a, b]; };
const named = function pick<T>(xs: T[]): T { return xs[0]; };
function outer(): number {
  function first<T>(xs: T[]): T { return xs[0]; }
  return first([1, 2]);
}
console.log(identity(1));
console.log(pair("a", 2)[1]);
console.log(named([7, 8]));
console.log(outer());
