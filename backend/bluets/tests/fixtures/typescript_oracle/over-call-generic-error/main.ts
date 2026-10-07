// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function choose<T>(value: T[]): T; declare function choose(value: string): string; export const answer: string = choose([42]);
