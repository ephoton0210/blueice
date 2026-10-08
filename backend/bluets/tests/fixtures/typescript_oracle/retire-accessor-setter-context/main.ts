// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box { get value(): number { return 1; } set value(value) { const n: number = value; } } const b = new Box(); b.value = 7;
