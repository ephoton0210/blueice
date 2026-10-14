// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class A { m(a: number): string { return 's'; } static s(a: string): number { return 1; } n(): void {} p(a: number): number; p(a: string): string; p(a: any): any { return a; } }
class D extends A { x(): number { return super.s('x'); } }
