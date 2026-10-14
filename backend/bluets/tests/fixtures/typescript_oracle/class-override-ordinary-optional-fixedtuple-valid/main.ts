// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    derived(...parts: [number, boolean]): number { return 1; }
    inherited(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    shifted(a?: number, ...parts: [boolean]): number { return 1; }
    reverse(...parts: [number, ...string[], boolean]): number { return 1; }
    viaUndefined(...parts: [undefined, boolean]): number { return 1; }
    viaOptional(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    static parse(...parts: [number, boolean]): number { return 1; }
}
class Child extends Base {
    derived(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    inherited(...parts: [number, boolean]): number { return 1; }
    shifted(...parts: [number, ...string[], boolean]): number { return 1; }
    reverse(a?: number, ...parts: [boolean]): number { return 1; }
    viaUndefined(a?: number, ...parts: [...string[], boolean]): number { return 1; }
    viaOptional(...parts: [undefined, boolean]): number { return 1; }
    static parse(a?: number, ...parts: [...string[], boolean]): number { return 1; }
}
