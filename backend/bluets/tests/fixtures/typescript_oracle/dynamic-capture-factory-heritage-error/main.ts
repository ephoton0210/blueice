// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function make<T>(value: T) { return class { item: T = value; }; } const Base = make(7); export const Derived = class extends Base {}; const result: string = new Derived().item;
