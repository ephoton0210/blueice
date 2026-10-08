// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class C { value = 3; read<T>(value: T): T { this.value; return value; } } const value: number = new C().read(3);
