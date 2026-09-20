// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const count: number = 41;
const exact: boolean = count + 1 === 42;
const bounded: boolean = (exact || false) && count < 42 && !false;
const positive: number = +count;
const signed: number = -count;
const inverted: number = ~count;
const selected: number = bounded ? count + 1 : count - 1;
const nested: number = false ? true ? count + 1 : count - 1 : count;
function enabled(value: number): boolean { return value >= 0 ? !false : false; }
console.log(`${exact}:${bounded}:${selected}:${enabled(-1)}:${signed}:${inverted}:${positive}:${nested}`);
