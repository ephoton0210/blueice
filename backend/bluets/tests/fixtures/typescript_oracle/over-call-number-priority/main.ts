// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function choose(value: number): 'wide'; declare function choose(value: 42): 'narrow'; export const answer: 'narrow' = choose(42);
