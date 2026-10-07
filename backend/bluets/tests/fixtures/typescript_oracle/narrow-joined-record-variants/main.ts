// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function read(flag: boolean): number { let value: { kind: 'text'; payload: string } | { kind: 'count'; payload: number }; if (flag) { value = { kind: 'text', payload: 'one' }; } else { value = { kind: 'count', payload: 2 }; } if (value.kind === 'count') { return value.payload; } return value.payload.length; }
