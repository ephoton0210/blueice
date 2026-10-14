// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

enum Kind { A, B }
class Item {
    kind: Kind = Kind.A;
    label(): string { return Kind[this.kind]; }
    matches(k: Kind): boolean { return this.kind === k; }
}
const i = new Item();
const b: boolean = i.matches(Kind.B);
