// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Box<T> { value: T; } function read<T>(value: Box<T>): T { return value.value; } export const answer: number = read({ value: 42 });
