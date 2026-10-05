// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function work() { const value: number = 1; }
export function bare() { return; }
export function absent() { return undefined; }
export function nil() { return null; }
export function fail() { throw new Error("bad"); }
