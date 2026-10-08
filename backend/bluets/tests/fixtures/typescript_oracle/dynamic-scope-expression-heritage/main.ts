// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const Base = class<T> { constructor(public value: T) {} }; const Derived = class extends Base<number> {}; const result: number = new Derived(7).value;
