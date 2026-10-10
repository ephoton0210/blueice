// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// retained ordinary comment
/** Public answer. */
export enum Shade {
    /** Public value. */
    Visible = 42,
    /** @internal */
    Hidden = 41,
    /** Public tail. */
    Last,
}
export const result: number = Shade.Visible;
export const text: string = "// literal /* retained */";
