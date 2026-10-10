// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// retained ordinary comment
/** @internal */
export function hidden(): number { return 1; }
/** Public answer. */
export const result: number = hidden() + 41;
export const text: string = "// literal /* retained */";
