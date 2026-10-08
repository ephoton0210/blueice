// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class B { value = 3; } export class C extends B { declare value: number; } console.log(new C().value, Object.hasOwn(new C(), "value"));
