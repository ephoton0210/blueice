// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class A { m(a: string): void; m(a: number): void; m(a: string | number): void {} }
class B extends A { m(a: string): number; m(a: number): string; m(a: string | number): number | string { return a as any; } }
class P { m(a: string): string; m(a: number): number; m(a: string | number): string | number { return a; } }
class Q extends P { m(a: string, b?: number): string; m(a: number, b?: number): number; m(a: string | number, b?: number): string | number { return a; } }
