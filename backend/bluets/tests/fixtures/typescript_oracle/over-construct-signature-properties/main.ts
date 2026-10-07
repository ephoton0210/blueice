// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Box { value: string | number; } type Factory = { readonly label: string; new (value: string): Box; new (value: number): Box; }; declare const factory: Factory; export const answer: Box = new factory(42); export const label: string = factory.label;
