// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export let answer: number = 20;
export function bump(): void { answer += 22; }
bump();
console.log(answer);
