// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    inherited(...parts: [number, ...string[], boolean]): number { return 1; }
    long(...parts: [number, ...string[], boolean]): number { return 1; }
    derived(...parts: [number, string, boolean]): number { return 1; }
    static parse(...parts: [number, ...string[], boolean]): number { return 1; }
}
class Child extends Base {
    inherited(...parts: [number, boolean]): number { return 1; }
    long(...parts: [number, string, boolean]): number { return 1; }
    derived(...parts: [number, ...string[], boolean]): number { return 1; }
    static parse(...parts: [number, string, boolean]): number { return 1; }
}
