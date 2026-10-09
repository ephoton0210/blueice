// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type { Item, Count as N } from "./dep.ts";
const item: Item = {value:42}; const count: N = item.value; console.log(count);
