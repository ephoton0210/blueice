// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function identity<T>(value: T): T { return value; }
const sum: number = identity<number>(41) + 1;
const difference: number = identity<number>(41) - 1;
const product: number = identity<number>(42) * 2;
const quotient: number = identity<number>(41) / 2;
const remainder: number = identity<number>(41) % 2;
const label: string = identity<string>('Ada') + ' Lovelace';
console.log(`${sum}:${difference}:${product}:${quotient}:${remainder}:${label}`);
