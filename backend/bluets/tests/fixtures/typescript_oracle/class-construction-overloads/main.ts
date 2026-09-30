// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    constructor(value: number);
    constructor(value: string);
    constructor(value: number | string) {}
}
const first: Box = new Box(1);
const second: Box = new Box('x');
