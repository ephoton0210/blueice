// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { private static n: number = 1; static get value(): number { return this.n; } static set value(value: string) { this.n = value.length; } } Box.value = "four"; console.log(Box.value);
