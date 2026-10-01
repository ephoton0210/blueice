// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

enum E { A, B }
namespace E {
    export function label(e: E): string { return e === E.A ? "a" : "b"; }
}
console.log(E.label(E.B), E.A);
