// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class B<T> { constructor(public value: T) {} } class C<T> extends B<T> {} class D extends C<string> {} const value: string = new D("ok").value;
