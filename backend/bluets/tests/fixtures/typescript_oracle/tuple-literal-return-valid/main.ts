// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
type Pair = [number, string];
export function basic(): [number, string] { return [1, "a"]; }
export function optional(): [number, string?] { return [1]; }
export function withRest(): [number, ...string[]] { return [1, "a", "b"]; }
export function spread(t: [string?]): [number, string?] { return [1, ...t]; }
export function named(): Pair { return [1, "a"]; }
export function locals(a: number): [number, number] { const b = a; return [a, b]; }
export function branch(flag: boolean): [number, string] { if (flag) { return [1, "a"]; } return [2, "b"]; }
export function arithmetic(a: number): [number, string] { return [a + 1, "x"]; }
export function nested(): [[number, string], boolean] { return [[1, "a"], true]; }
