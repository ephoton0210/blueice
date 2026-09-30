// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(value: number, label: string): number { return value; }
    static parse(value: number, label: string): number { return value; }
}
class Shorter extends Base {
    read(value: number): number { return value; }
    static parse(value: number): number { return value; }
}
class Wider extends Base {
    read(value: number, label: string, extra?: boolean): number { return value; }
    static parse(value: number, label: string, extra: number = 1): number { return value; }
}
