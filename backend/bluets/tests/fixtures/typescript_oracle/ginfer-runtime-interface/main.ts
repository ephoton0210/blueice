// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export interface Box<T = number> { value: T; } export function read<T>(value: Box<T>): T { return value.value; } console.log(read({ value: 42 }));
