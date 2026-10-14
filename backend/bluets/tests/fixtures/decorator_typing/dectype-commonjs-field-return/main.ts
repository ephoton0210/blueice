// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(value: any, context: any): (n: number) => string { return (n: number): string => "bad"; }
class C { @d value: number = 42; }
export const result: number[] = [new C().value];
