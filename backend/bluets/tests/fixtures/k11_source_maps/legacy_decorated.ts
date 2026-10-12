// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function classDecor(value: any): any { console.log("class"); return value; }
function methodDecor(target: any, key: string, descriptor: any): any { console.log("method"); return descriptor; }

@classDecor
class Example {
    @methodDecor
    method() { return 42; }
}
console.log(new Example().method());
