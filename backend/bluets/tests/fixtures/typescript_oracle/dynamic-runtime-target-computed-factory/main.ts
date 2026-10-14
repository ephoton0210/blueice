// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export {}; let count = 0; class Keys { static get value(): "first" { return (++count === 1 ? "first" : "second") as "first"; } } function make() { return class { [Keys.value]: number = 7; }; } const A = make(); const B = make(); const a: any = new A(); const b: any = new B(); console.log(a.first, b.second, count);
