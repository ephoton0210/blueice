// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Visitor {
    visit(kind: 'text', callback: (value: string) => void): void;
    visit(kind: 'count', callback: (value: number) => void): void;
}

declare const visitor: Visitor;
declare const kind: 'text' | 'count';
function onText(value: string): void {}
function onCount(value: number): void {}

visitor.visit(kind, onText);
visitor.visit('other', onText);
visitor.visit('text', onCount);
