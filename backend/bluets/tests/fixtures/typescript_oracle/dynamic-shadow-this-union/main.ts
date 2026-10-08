// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class C<T> { constructor(public value: T) {} read(): T | undefined { return this.value; } } const n: number | undefined = new C(7).read();
