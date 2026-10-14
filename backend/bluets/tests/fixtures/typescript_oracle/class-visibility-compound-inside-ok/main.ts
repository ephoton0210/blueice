// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    private secret: number = 1;
    protected shared: number = 2;
    read(other: Box, list: Box[]): number {
        return (this).secret + other.secret * this.shared + list[0].secret + new Box().secret;
    }
    static make(): number { return new Box().secret + Box.make2().secret; }
    static make2(): Box { return new Box(); }
}
class Sub extends Box {
    twice(): number { return this.shared * 2 + new Sub().shared; }
}
const t: number = new Sub().twice();
