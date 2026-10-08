// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export let count = 0; class Keys { static get value(): "value" { count++; return "value"; } } export class C { [Keys.value] = 7; } const first: any = new C(); const second: any = new C(); console.log(count, first["value"], second["value"]);
