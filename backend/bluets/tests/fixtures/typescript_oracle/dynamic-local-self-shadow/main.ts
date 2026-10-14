// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function make<T>(value: T): T { const C = class Inner { item: T = value; clone<T>(): Inner { return new Inner(); } }; return new C().clone<string>().item; } const n: number = make(7);
