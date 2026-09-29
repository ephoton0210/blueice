// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base { read(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; } }
class Child extends Base {
    read(a: number, b: string): number { return a; }
}
