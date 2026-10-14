// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Identity = <T = number>(value: T) => T; const identity: Identity = <T>(value: T): T => value; export const answer: number = identity(42);
