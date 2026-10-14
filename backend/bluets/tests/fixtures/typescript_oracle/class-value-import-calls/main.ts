// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Box as LocalBox } from './box.ts';
const box = new LocalBox(1);
const readValue: number = box.read(2);
const made: LocalBox = LocalBox.make(3);
const madeValue: number = made.read(4);
