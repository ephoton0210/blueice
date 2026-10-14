// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function over(p: [number]): number;
function over(p: [string, string]): string;
function over(p: any): any { return p; }
over([true]);
