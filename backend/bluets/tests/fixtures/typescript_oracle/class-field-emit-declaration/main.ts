// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export class Shape {
    count: number = 0;
    static total: number = 1;
    label = "x";
    readonly id: number = 1;
    tag?: string;
    name!: string;
    static readonly kind = "shape";
    describe(): string { return this.label; }
}
