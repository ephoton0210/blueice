// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(target: any, key: string, descriptor: any): TypedPropertyDescriptor<(n: number) => number> { return { value: (n: number): number => n + 1 }; }
class C { @d m(n: number): number { return n; } }
export const result: number[] = [new C().m(41)];
