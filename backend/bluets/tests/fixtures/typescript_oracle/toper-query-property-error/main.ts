// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const source: {name: string} = {name: 'x'}; type Copy = typeof source.name; export const value: Copy = 1;
