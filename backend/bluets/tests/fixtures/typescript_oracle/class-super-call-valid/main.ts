// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class A { constructor(x: number, y: string) {} }
class D extends A { constructor() { super(1, 'a'); } }
class E extends A { constructor(a: number) { const k = a + 1; super(k, 'a'); } }
class F extends A { constructor() { super(1, 'a'); super(2, 'b'); } }
class G extends A {}
class H { constructor() {} }
class O { constructor(x: number); constructor(x: string); constructor(x: any) {} }
class P extends O { constructor() { super(1); } }
class Q extends O { constructor() { super('s'); } }
class R extends A { constructor(flag: boolean) { if (flag) { super(1, 'a'); } else { super(2, 'b'); } } }
