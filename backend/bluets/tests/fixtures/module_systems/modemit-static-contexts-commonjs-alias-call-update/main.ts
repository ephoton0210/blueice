// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import {live, bump} from "./dep"; export const before = live; const returned = bump(); export const result = [live, returned];
