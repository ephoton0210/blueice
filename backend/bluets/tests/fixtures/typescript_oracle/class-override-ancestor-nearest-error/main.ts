// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Grand { read(value: number | string): number { return 1; } }
class Middle extends Grand { read(value: number): number { return value; } }
class Child extends Middle {
    read(value: string): number { return 1; }
}
