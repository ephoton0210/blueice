// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class A { m(a: string): string; m(a: number): number; m(a: string | number): string | number { return a; } }
class B extends A { m(a: string): string; m(a: number): number; m(a: string | number): string | number { return a; } }
class C extends A { m(a: string): string; m(a: number): number; m(a: boolean): boolean; m(a: string | number | boolean): string | number | boolean { return a; } }
class D { m(a: string): string { return a; } }
class E extends D { m(a: string): string; m(a: number): number; m(a: string | number): string | number { return a; } }
class S { static s(a: string): string; static s(a: number): number; static s(a: string | number): string | number { return a; } }
class T extends S { static s(a: string): string; static s(a: number): number; static s(a: string | number): string | number { return a; } }
