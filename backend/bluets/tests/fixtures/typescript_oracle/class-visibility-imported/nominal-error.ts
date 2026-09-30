// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Box } from "./box.ts";
class Local {
    private secret: number = 1;
    protected shared: number = 2;
    public open: number = 3;
    protected peek(): number { return 1; }
}
const b: Box = new Local();
