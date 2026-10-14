// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type { Marker } from "./augment"; import type { Item } from "./dep"; export type Witness = Marker; export const answer: Item = { value: 42, tag: "ok" }; console.log(answer.value);
