// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base { static parse(a: number, ...parts: string[]): number { return a; } }
class Child extends Base {
    static parse(a: number, ...parts: [...middle: boolean[], end: string]): number { return a; }
}
