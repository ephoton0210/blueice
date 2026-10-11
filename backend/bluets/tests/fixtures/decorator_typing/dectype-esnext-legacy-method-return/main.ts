// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(target: any, key: string, descriptor: any): TypedPropertyDescriptor<(n: number) => string> { return { value: (n: number): string => "bad" }; }
class C { @d m(n: number): number { return n; } }
export const result: number[] = [new C().m(42)];
