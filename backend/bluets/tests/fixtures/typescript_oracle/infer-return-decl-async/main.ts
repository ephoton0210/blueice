// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export async function count() { return 1; }
export async function work() { return; }
export async function flattened() { return Promise.resolve<number>(1); }
export async function awaited() { return await Promise.resolve<number>(1); }
