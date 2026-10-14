// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function make<T>(value: T) { return class { item: T = value; read(): T { return this.item; } }; } const C = make(7); console.log(new C().read());
