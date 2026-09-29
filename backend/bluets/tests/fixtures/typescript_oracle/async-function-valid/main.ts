// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
async function base(n: number): Promise<number> { return n; }
export async function a(): Promise<void> { await base(1); }
export async function b(flag: boolean): Promise<number> { if (flag) { return 1; } return await base(2); }
export async function c() { return 1; }
export async function d(): Promise<number> { return (await base(1)) + 1; }
export async function e(): Promise<number> { return await 5; }
export function f(): Promise<number> { return base(1); }
export const g: Promise<number> = base(1);
