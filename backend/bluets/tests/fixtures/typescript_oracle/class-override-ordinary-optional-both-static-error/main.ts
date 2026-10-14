// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base { static parse(a?: number, ...parts: [...string[], boolean]): number { return 1; } }
class Child extends Base {
    static parse(a?: string, ...parts: [...string[], boolean]): number { return 1; }
}
