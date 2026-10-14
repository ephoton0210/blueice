// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function get<T, K extends keyof T>(value: T, key: K): T[K] { return value[key]; } export const answer = get({ value: 42 }, "missing");
