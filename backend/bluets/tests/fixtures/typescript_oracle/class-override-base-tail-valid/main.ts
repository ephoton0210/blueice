// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(...parts: [first: number, ...tail: string[]]): number { return parts[0]; }
    static parse(...parts: [first: number, ...tail: string[]]): number { return parts[0]; }
}
class Child extends Base {
    read(a: number, b: string, c: string): number { return a; }
    static parse(a: number, ...tail: string[]): number { return a; }
}
