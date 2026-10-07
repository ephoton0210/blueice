// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function first<T>(...values: T[]): T { return values[0]; } export const answer: number = first(1, 2);
