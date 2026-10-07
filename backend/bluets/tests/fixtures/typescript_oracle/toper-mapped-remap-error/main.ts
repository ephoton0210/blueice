// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Renamed<T> = {[K in keyof T as `get_${K & string}`]: T[K]}; export const value: Renamed<{name: string}> = {name: 'x'};
