// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Box } from "./lib.ts";
const b: Box = new Box();
const s: string = b.extra;
console.log(b.v, s === undefined);
