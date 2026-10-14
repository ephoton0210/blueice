// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Shape { area(): number }
class Shape {
    w: number = 2;
    perimeter(): number { return this.w * 4; }
}
const s: Shape = new Shape();
const p: number = s.perimeter();
console.log(p);
