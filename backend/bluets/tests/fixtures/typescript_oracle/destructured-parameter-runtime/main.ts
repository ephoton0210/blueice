// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
interface Point { x: number; y: number; label?: string }
const norm = ({ x, y }: Point): number => x * x + y * y;
const nameOf = function ({ label }: Point): string { return label ?? "none"; };
const first = ([a, b]: [number, string]): string => b + a;
function describe({ x, y }: Point, [k]: [string]): string {
  function inner({ x }: { x: number }): number { return x + 1; }
  return k + ":" + inner({ x }) + ":" + y;
}
const renamed = ({ x: px, y: py = 5 }: { x: number; y?: number }): number => px + py;
console.log(norm({ x: 3, y: 4 }));
console.log(nameOf({ x: 0, y: 0 }));
console.log(nameOf({ x: 0, y: 0, label: "L" }));
console.log(first([1, "s"]));
console.log(describe({ x: 1, y: 2 }, ["k"]));
console.log(renamed({ x: 1 }));
