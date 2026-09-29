// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
type Pair = [number, string];
class Base { method(...args: [boolean, number, string]): void {} }
class Bad extends Base { method(...args: [boolean, ...Pair, number]): void {} }
