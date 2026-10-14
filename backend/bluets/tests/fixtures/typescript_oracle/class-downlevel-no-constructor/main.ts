// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Plain { a = 1; }
class Child extends Plain { b: number = this.a + 1; c: string = "c"; }
class Grand extends Child { d: number = this.b + this.a; }
class Empty {}
class EmptyChild extends Empty { z: number = 9; }
const g = new Grand();
console.log(JSON.stringify(g), Object.keys(new EmptyChild()).join(","), Object.keys(new Empty()).length);
