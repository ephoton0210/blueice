// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Point {
    z: number = 5;
    constructor(public x: number, private y: number, protected readonly w: number = 3, readonly v?: string) {}
    sum(): number { return this.x + this.y + this.w + this.z; }
    label(): string { return this.v ?? "none"; }
}
const p = new Point(1, 2);
const n: number = p.x + p.sum();
const s: string = p.label();
