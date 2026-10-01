// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    class H { v: number = 1; }
    export class Exposed { v: number = 2; }
}
class D extends N.H { }
console.log(new D());
