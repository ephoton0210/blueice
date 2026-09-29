// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class A {
  m(): [number, string] { return [1, "a"]; }
  static s(flag: boolean): [number, string?] { if (flag) { return [1]; } return [2, "b"]; }
}
