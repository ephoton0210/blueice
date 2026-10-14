// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Renamed as Local } from './box.ts';
const child = new Local(1);
const text: string = child.label(1);
const parsed: number = Local.parse(1);
