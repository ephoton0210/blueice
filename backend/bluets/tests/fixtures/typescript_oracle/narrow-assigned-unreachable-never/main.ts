// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function read(): number { let value: string | number = 1; if (typeof value === 'string') { const impossible: never = value; } return value; }
