// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const stream: Generator<number, string, undefined>; const seen: number[] = []; let step = stream.next(); while (!step.done) { step = { done: true, value: "done" }; seen.push(step.value); break; }
