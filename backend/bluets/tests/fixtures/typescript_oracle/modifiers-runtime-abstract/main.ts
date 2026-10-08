// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export abstract class Base { abstract read(): number; twice(): number { return this.read() * 2; } } export class Child extends Base { override read(): number { return 3; } } console.log(new Child().twice());
