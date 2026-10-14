// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

abstract class Base { abstract get value(): number; abstract set value(v: number); } class Child extends Base { private stored: number = 1; get value(): number { return this.stored; } set value(v: number) { this.stored = v; } }
