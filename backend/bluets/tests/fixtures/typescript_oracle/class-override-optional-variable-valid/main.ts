// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    fromOptional(...parts: [string?, ...string[]]): number { return 1; }
    fromMiddle(...parts: [...string[], string]): number { return 1; }
    shifted(...parts: [string?, string?, ...string[]]): number { return 1; }
    static parse(...parts: [...string[], string]): number { return 1; }
}
class Child extends Base {
    fromOptional(...parts: [...string[], string]): number { return 1; }
    fromMiddle(...parts: [string?, ...string[]]): number { return 1; }
    shifted(...parts: [...string[], string]): number { return 1; }
    static parse(...parts: [string?, ...string[]]): number { return 1; }
}
