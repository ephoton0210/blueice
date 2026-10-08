// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class B<T> { read(value: T): T { return value; } } class C extends B<number> { override read(value: string): string { return value; } }
