// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const first: unique symbol = Symbol('first'); const second: unique symbol = Symbol('second'); export const value: typeof first = second;
