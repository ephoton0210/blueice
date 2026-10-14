// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// ECMAScript 2023 extends weak references to non-registered symbols. The
// registered-symbol restriction is checked by the runtime.
interface WeakKeyTypes {
    symbol: symbol;
}
