// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Holder { readonly #value: number = 1; run(): void { const other = {"#value": 1}; [this.#value] = [2]; } }
