// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(value: any, context: any): (n: number) => void { return (n: number): void => {}; }
class C { @d set value(n: number) {} answer: number = 42; }
export const result: number[] = [new C().answer];
