// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export const enum Color { Red, Green = 5, Blue }
export enum Mode { Fast = "fast", Slow = "slow" }
export const enum Hidden { X = 1 }
const enum Local { A = 9 }
export function localA(): number { return Local.A; }
