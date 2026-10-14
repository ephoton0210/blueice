// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import { Base } from './box.ts';
class Child extends Base { read(): string { return this.label(1); } }
const child = new Child();
const text: string = child.label(2);
