// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
async function base(n: number): Promise<number> { return n + 1; }
const arrow = async (n: number): Promise<number> => (await base(n)) * 2;
const expr = async function (n: number): Promise<string> { return "e" + (await base(n)); };
const named = async function twice(n: number): Promise<number> { return n * 2; };
const obj = { async m(n: number): Promise<number> { return await base(n); } };
async function outer(): Promise<string> {
  async function local(n: number): Promise<number> { return await arrow(n); }
  const a = await local(1);
  const b: string = await expr(2);
  const c = await named(3);
  const d = await obj.m(4);
  return a + ":" + b + ":" + c + ":" + d;
}
outer().then((s) => console.log(s));
