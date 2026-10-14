// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Grand { static parse(value: number): number { return value; } }
class Middle extends Grand {}
class Child extends Middle {
    static parse(value: string): number { return 1; }
}
