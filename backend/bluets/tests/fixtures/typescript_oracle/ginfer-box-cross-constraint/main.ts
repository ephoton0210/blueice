// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Pair<T, U extends T> { constructor(public first: T, public second: U) {} } export const answer: number = new Pair<number, number>(1, 2).second;
