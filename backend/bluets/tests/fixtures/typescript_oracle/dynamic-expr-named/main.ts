// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const C = class Inner { value = 3; clone(): Inner { return new Inner(); } }; const value: number = new C().clone().value;
