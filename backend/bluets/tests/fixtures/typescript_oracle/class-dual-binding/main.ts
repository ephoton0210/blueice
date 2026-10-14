// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Reader {
    read(): number { return 1; }
}
const instance: Reader = Reader.prototype;
const constructorSide: { prototype: Reader } = Reader;
const methodShape: { read(): number } = Reader.prototype;
