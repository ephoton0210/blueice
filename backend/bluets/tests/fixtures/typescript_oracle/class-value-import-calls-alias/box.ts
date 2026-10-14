// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    constructor(value: number) {}
    read(value: number): number { return value; }
    static make(value: number): Box { return new Box(value); }
}
export { Box as PublicBox };
