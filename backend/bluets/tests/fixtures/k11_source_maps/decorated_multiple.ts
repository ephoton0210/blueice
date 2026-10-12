// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function outer(value: any, context: any) { console.log("outer"); return value; }
function inner(value: any, context: any) { console.log("inner"); return value; }
function classDecor(value: any, context: any) { console.log("class"); return value; }

@classDecor
class Example {
    @outer
    @inner
    method() { return 42; }
}
console.log(new Example().method());
