// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export {}; class Holder { C: { new(): { item: number }; value: number } = class { static value: number = 7; item: number = 5; }; } const C = new Holder().C; console.log(C.name, C.value, new C().item);
