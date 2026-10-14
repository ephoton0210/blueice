// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let closed = 0;
const values = {
  [Symbol.iterator]() {
    return {
      next() { return { done: false, value: 42 }; },
      return() { closed++; return { done: true, value: 0 }; }
    };
  }
};
try { for (const value of values) { if (value === 42) { throw value; } } }
catch (error) { console.log(error, closed); }
export const result = closed;
