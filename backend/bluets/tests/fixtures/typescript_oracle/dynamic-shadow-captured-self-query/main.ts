// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function make<T>(value: T) { return class Inner { item: T = value; copy<T>(): typeof Inner { return Inner; } }; } const C = make(7); const D = new C().copy<string>(); const n: number = new D().item;
