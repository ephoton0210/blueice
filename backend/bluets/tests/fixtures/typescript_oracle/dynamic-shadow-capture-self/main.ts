// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function make<T>(value: T) { return class Inner { item: T = value; clone<T>(): Inner { return new Inner(); } }; } const C = make(7); const result: number = new C().clone<string>().item;
