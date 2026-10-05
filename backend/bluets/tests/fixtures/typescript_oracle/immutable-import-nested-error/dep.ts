// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export const value: number = 1;
export let mutable: number = 1;
export const holder: { value: number } = { value: 1 };
export function read(): number { return value; }
export default value;
