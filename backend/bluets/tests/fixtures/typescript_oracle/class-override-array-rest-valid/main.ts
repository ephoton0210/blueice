// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(prefix: number, ...values: string[]): number { return prefix; }
    static parse(prefix: number, ...values: string[]): number { return prefix; }
}
class Child extends Base {
    read(prefix: number, ...values: string[]): number { return prefix; }
    static parse(prefix: number, ...values: string[]): number { return prefix; }
}
