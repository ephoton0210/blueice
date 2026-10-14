// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// retained ordinary comment
/** Public answer. */
export class Box {
    /** @internal */
    hidden: number = 1;
    /** Public value. */
    value: number = 41;
    /** @internal */
    secret(): number { return 1; }
    /** Public method. */
    answer(): number { return this.value + 1; }
}
export const result: number = new Box().answer();
export const text: string = "// literal /* retained */";
