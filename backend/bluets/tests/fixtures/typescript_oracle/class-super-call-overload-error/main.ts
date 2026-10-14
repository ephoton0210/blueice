// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class O { constructor(x: number); constructor(x: string); constructor(x: any) {} }
class E extends O { constructor() { super(true); } }
