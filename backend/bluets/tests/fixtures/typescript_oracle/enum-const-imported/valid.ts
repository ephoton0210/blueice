// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Color, Mode, localA } from "./colors.ts";
const c: Color = Color.Blue;
const n: number = Color.Green + c + localA();
const m: Mode = Mode.Fast;
const s: string = m;
const all: string = `${Color.Red}-${Mode.Slow}`;
console.log(c, n, m, s, all, Mode.Slow, Color.Blue);
