// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type OnlyName<T> = {[K in keyof T as K extends 'name' ? K : never]: T[K]}; export const value: OnlyName<{name: string; count: number}> = {name: 'x'};
