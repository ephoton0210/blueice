// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function takesString(value: string): void {} export function read(box: { value: string | number }): void { if (typeof box.value === 'string') { box.value = 1; takesString(box.value); } }
