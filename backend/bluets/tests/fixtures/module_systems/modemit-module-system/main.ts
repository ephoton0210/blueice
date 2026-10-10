// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import base, {current, bump, unbound} from "./dep"; import * as values from "./dep"; import {forwarded} from "./barrel"; export const before = current; bump(); export const result = [current, values.current, forwarded, base(), unbound()];
