// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Box } from "./box.ts";
class Sub extends Box {
    total(): number { return this.shared + this.open + this.peek(); }
}
const n: number = new Sub().total() + new Box().open;
