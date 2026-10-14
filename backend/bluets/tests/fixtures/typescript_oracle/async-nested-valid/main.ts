// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
async function base(n: number): Promise<number> { return n; }
export const a = async (n: number): Promise<number> => await base(n);
export const b = async (n: number) => n;
export const c = async function (n: number): Promise<void> { await base(n); };
export const d = { async m(): Promise<number> { return 1; } };
export async function e(): Promise<number> {
  const xs = [1, 2].map(async (n: number): Promise<number> => n);
  async function inner(): Promise<number> { return await base(1); }
  return await inner();
}
export const f: (n: number) => Promise<number> = async (n: number): Promise<number> => n;
