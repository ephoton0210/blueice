// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type { Box } from './box.ts';
declare const box: Box;
const method: (value: number) => number = box.read;
const value: number = box.read(1);
