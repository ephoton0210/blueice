// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(...parts: [number, string?]): number { return 1; }
    shorten(...parts: [number, string?]): number { return 1; }
    widen(...parts: [number, string]): number { return 1; }
    static parse(a: number, ...parts: [string?]): number { return a; }
}
class Child extends Base {
    read(a: number, b: string): number { return a; }
    shorten(...parts: [number]): number { return 1; }
    widen(...parts: [number, string?]): number { return 1; }
    static parse(...parts: [number, string?]): number { return parts[0]; }
}
