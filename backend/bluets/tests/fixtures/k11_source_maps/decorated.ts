// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function decoSite(value: any, context: any) { return value; }
class Example {
    @decoSite
    method() { return 42; }
}
console.log(new Example().method());
