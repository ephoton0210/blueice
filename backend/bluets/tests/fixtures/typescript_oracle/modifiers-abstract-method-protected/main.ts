// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

abstract class Base { protected abstract read(): number; value(): number { return this.read(); } } class Child extends Base { protected read(): number { return 1; } }
