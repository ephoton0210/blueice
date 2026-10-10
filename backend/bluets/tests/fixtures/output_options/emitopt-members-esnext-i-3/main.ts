// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// retained ordinary comment
/** Public answer. */
export interface Shape {
    /** @internal */
    hidden: number;
    /** Public value. */
    value: number;
    /** @internal */
    secret(): number;
    /** Public method. */
    answer(): number;
}
export const result: number = 42;
export const text: string = "// literal /* retained */";
