// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Brand {
    #tag: number = 1;
    static has(value: any): boolean { return #tag in value; }
    check(value: any): boolean { return #tag in value; }
}
const b: boolean = Brand.has(new Brand());
