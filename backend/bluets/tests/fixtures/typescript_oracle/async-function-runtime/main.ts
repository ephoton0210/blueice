// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
async function double(n: number): Promise<number> { return n * 2; }
async function run(): Promise<string> {
  const a = await double(2);
  const b: number = await double(a);
  return "r" + b;
}
run().then((s) => console.log(s));
