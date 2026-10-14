// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let count = 0; const key = "value"; export class C { [key] = 3; [++count](): number { return count; } static [key] = 5; } console.log(count, new C()[1](), new C().value, C.value);
