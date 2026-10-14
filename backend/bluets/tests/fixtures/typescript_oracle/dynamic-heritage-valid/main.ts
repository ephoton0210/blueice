// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class B<T> { constructor(public value: T) {} read(): T { return this.value; } } class C<T> extends B<T> {} const c: C<number> = new C(3); const value: number = c.read();
