// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function read<T extends {answer: number}>(value: T): T {return value;} export const value = read({answer: 42, extra: 1});
