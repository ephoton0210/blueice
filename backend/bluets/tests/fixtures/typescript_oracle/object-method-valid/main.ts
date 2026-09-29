// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export function make(k: number) {
  return {
    scale(n: number): number { return n * k; },
    name(): string { return "x"; },
  };
}
export const o = {
  twice(n: number): number { return n + n; },
  get size(): number { return 1; },
};
export const r: number = o.twice(2) + o.size;
