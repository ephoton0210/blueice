// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare namespace Lib {
    const version: number;
    function describe(n: number): string;
    interface Options { verbose: boolean }
}
const o: Lib.Options = { verbose: true };
declare const fake: number;
console.log(o.verbose, fake);
