// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function run(seed: number): number {
  const fixed: number = seed;
  let value: number = fixed;
  value += 2;
  value++;
  --value;
  const holder: { value: number; readonly label: string } = { value, label: "ok" };
  holder.value++;
  let other: number = 0;
  [other] = [holder.value];
  return other;
}
export const result: number = run(3);
console.log(result);
