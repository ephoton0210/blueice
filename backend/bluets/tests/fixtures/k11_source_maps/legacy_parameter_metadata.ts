// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function methodDecor(target: any, key: string, descriptor: any): any {
    console.log("method");
    return descriptor;
}
function parameterDecor(target: any, key: string, index: number): void {
    console.log("parameter", index);
}
class Example {
    @methodDecor
    method(@parameterDecor value: number): number { return value + 1; }
}
console.log(new Example().method(41));
