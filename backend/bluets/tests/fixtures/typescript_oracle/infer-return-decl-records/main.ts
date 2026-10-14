// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function entry() { return { value: 1 }; }
export function values() { return [1, 2]; }
export function literal(value: 'yes') { return { value }; }
