// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type { Public, Alias as ViaStar } from './second.ts';
declare const first: Public;
declare const second: ViaStar;
const firstText: string = first.label(1);
const secondText: string = second.label(2);
