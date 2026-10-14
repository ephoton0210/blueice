// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    constructor(value: number) {}
    read(): number { return 1; }
}
const inferred = new Box(1);
const shape: { read(): number } = inferred;
class Empty {}
const empty: Empty = new Empty();
