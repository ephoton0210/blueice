// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Types {
    export interface Point { x: number; y: number }
    export type Pair = [number, number];
}
const p: Types.Point = { x: 1, y: 2 };
const pair: Types.Pair = [p.x, p.y];
console.log(pair);
