// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base { read(prefix: number, ...labels: string[]): number { return prefix; } }
class Child extends Base {
    read(prefix: number, first: string, second: string, ...labels: string[]): number { return prefix; }
}
