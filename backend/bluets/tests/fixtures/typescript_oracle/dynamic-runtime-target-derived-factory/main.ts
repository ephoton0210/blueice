// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export {}; class Base { item: number = 5; } function make(value: number) { return class extends Base { static value: number = value; }; } const A = make(3); const B = make(4); console.log(A.value, B.value, new A().item, new B().item);
