// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(prefix: number, ...labels: string[]): number { return prefix; }
    static parse(prefix: number, ...labels: string[]): number { return prefix; }
}
class Shorter extends Base {
    read(prefix: number): number { return prefix; }
    static parse(prefix: number): number { return prefix; }
}
class Optional extends Base {
    read(prefix: number, label?: string): number { return prefix; }
    static parse(prefix: number, label?: string): number { return prefix; }
}
class LongerBase {
    read(prefix: number, label: string, ...active: boolean[]): number { return prefix; }
}
class Fewer extends LongerBase {
    read(prefix: number): number { return prefix; }
}
