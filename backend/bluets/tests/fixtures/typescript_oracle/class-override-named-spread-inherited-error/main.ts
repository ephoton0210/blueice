// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
type Pair = [number, string];
type Args = [boolean, ...Pair];
class Base { method(...args: Args): void {} }
class Bad extends Base { method(...args: [boolean, string, string]): void {} }
