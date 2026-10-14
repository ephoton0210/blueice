// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    fixed(a: undefined, b: boolean): number { return 1; }
    array(a: undefined, ...parts: string[]): number { return 1; }
    inheritedFixed(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    inheritedArray(a?: number, ...parts: [...string[], string]): number { return 1; }
}
class Child extends Base {
    fixed(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    array(a?: number, ...parts: [...string[], string]): number { return 1; }
    inheritedFixed(a: undefined, b: boolean): number { return 1; }
    inheritedArray(a: undefined, ...parts: string[]): number { return 1; }
}
