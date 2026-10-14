// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Frozen<T> = {readonly [K in keyof T]: T[K]}; declare const input: Frozen<number[]>; export const value: number[] = input;
