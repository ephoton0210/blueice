// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Account { id: string }
function identity<T>(value: T): T { return value; }
const accountName: string = identity('Ada');
const account: Account = { id: accountName };
function label(value: Account): string { return value.id; }
console.log(label(account));
