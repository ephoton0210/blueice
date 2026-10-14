// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class C { read<T>(value: T): T { return value; } static pair<T>(value: T): T { return value; } } console.log(new C().read(3), C.pair("ok"));
