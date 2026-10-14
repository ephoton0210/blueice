// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(...parts: [first: number, second: string]): number { return parts[0]; }
    static parse(a: number, ...parts: [value?: string]): number { return a; }
}
class Child extends Base {
    read(...parts: [left: number, right: string]): number { return parts[0]; }
    static parse(...parts: [key: number, text?: string]): number { return parts[0]; }
}
