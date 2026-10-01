// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Geo } from "./lib.ts";
import type { Only, Shape } from "./lib.ts";
const s: Shape = { sides: 4 };
const a: number = Geo.area(s);
const p: Geo.Point = new Geo.Point();
const k: Geo.Kind = Geo.Kind.Round;
const d: number = Geo.Deep.depth;
const t: Geo.Deep.Tag = { t: "x" };
const b: Geo.Box = { shape: s };
const id: Geo.Id = 7;
const o: Only.T = { v: 1 };
const i: number = Geo.internal().i;
Geo.bump();
console.log(a, p.dist(), k, d, t.t, b.shape.sides, id, o.v, Geo.counter, i);
