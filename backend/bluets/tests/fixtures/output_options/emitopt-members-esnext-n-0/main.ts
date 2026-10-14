// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// retained ordinary comment
/** Public answer. */
export namespace Space {
    /** @internal */
    export function hidden(): number { return 1; }
    /** Public value. */
    export const value: number = hidden() + 41;
}
export const result: number = Space.value;
export const text: string = "// literal /* retained */";
