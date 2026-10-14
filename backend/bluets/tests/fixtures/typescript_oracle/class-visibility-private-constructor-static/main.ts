// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Singleton {
    private static made: number = 0;
    label: string;
    private constructor(label: string) { this.label = label; }
    static create(): Singleton {
        Singleton.made = Singleton.made + 1;
        return new Singleton("s");
    }
}
const one: Singleton = Singleton.create();
