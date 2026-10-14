// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Geo as G } from "./lib.ts";
import type { Shape } from "./lib.ts";
const s: Shape = { sides: 3 };
const area: number = G.area(s);
const p: G.Point = new G.Point();
const k: G.Kind = G.Kind.Flat;
console.log(area, p.dist(), k, G.Deep.depth);
