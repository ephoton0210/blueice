// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Visitor {
    visit(kind: 'text', callback: (value: string) => void): void;
    visit(kind: 'count', callback: (value: number) => void): void;
}

let observed: number = 0;
function onText(value: string): void { if (value === 'A') { observed += 1; } }
function onCount(value: number): void { observed += value; }
function dispatch(kind: string, listener: any): void {
    if (kind === 'text') { listener('A'); }
    else { listener(2); }
}

const visitor: Visitor = { visit: dispatch };
visitor.visit('text', onText);
visitor.visit("count", onCount);
console.log(observed);
