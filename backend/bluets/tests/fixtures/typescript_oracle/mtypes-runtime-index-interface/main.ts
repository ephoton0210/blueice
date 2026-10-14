// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Table {[key: string]: number} const table: Table = {answer: 42}; export function read(): number {return table['answer'];} console.log(read());
