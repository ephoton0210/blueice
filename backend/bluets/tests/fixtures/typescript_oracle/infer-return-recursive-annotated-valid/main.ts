// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function count(value: number): number { if (value === 0) { return 0; } return count(value - 1); } const result: number = count(2);
