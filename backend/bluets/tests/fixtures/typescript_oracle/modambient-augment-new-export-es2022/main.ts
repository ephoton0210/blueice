// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type { NewItem } from "./dep"; import type { Marker } from "./augment"; export type Witness = Marker; export const answer: NewItem = { value: 42 }; console.log(answer.value);
