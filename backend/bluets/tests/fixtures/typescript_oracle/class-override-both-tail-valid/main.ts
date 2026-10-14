// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(...parts: [first: number, ...tail: string[]]): number { return parts[0]; }
    static parse(a: number, ...parts: [text: string, ...tail: boolean[]]): number { return a; }
}
class Child extends Base {
    read(...parts: [first: number, second: string, ...tail: string[]]): number { return parts[0]; }
    static parse(...parts: [first: number, text: string, ...tail: boolean[]]): number { return parts[0]; }
}
