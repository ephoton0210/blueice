// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Converter {
    static parse(value: string): string;
    static parse(value: number): number;
    static parse(value: any): any { return value; }
}
const invalid = Converter.parse(true);
