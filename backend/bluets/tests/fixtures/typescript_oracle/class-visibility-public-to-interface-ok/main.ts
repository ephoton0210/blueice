// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface HasValue { value: number }
class Box {
    public value: number = 1;
    private hidden: number = 2;
}
const h: HasValue = new Box();
