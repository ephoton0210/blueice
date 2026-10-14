// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Named { name: string; } export interface Box extends Named { extra: string; } export class Box { value: number = 1; } const b: Box = new Box(); const s: string = b.name; console.log(b.value, s === undefined);
