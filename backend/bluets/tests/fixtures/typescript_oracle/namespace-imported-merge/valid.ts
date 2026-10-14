// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { K, E, g } from "./lib.ts";
const n: number = K.s + K.t;
const k: K = new K();
const made: K = K.make();
const e: E = E.B;
const l: string = E.label(e);
const r: number = g(1);
const m: string = g.meta;
console.log(n, k.v, made.v, e, l, r, m);
