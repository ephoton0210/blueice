// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function risky(n: number): string {
  try {
    if (n) { throw 1; }
    return "ok";
  } catch (e: unknown) {
    return "caught";
  } finally {
    console.log("done");
  }
}
console.log(risky(0));
console.log(risky(1));
