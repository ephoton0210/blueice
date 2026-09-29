// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type { PublicBox as LocalBox } from './box.ts';
declare const box: LocalBox;
const cloned: LocalBox = box.clone();
