// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    public value: number = 1;
    public static count: number = 2;
    public read(): number { return this.value; }
}
const n: number = new Box().read() + Box.count;
