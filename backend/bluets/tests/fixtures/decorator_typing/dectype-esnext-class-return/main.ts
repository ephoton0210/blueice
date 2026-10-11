// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Other { different: string = "bad"; }
function d(value: any, context: any): typeof Other { return Other; }
@d class C { value: number = 42; }
export const result: number[] = [new C().value];
