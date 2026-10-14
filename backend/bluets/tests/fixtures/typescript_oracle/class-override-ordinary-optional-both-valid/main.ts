// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    required(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    optional(a: number, ...parts: [...string[], boolean]): number { return 1; }
    tuple(...parts: [number, ...string[], boolean]): number { return 1; }
    viaUndefined(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    viaOptional(a: undefined, ...parts: [...string[], boolean]): number { return 1; }
    mixed(a?: number, ...parts: [...string[], string]): number { return 1; }
    static parse(a?: number, ...parts: [...string[], boolean]): number { return 1; }
}
class Child extends Base {
    required(a: number, ...parts: [...string[], boolean]): number { return 1; }
    optional(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    tuple(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    viaUndefined(a: undefined, ...parts: [...string[], boolean]): number { return 1; }
    viaOptional(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    mixed(a?: number, ...parts: [string?, ...string[]]): number { return 1; }
    static parse(a: number, ...parts: [...string[], boolean]): number { return 1; }
}
