// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base { static parse(a: number, ...rest: boolean[]): number { return a; } }
class Child extends Base {
    static parse(...parts: [first: number, ...tail: string[]]): number { return parts[0]; }
}
