// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(value: typeof C, context: ClassDecoratorContext<typeof C>): typeof C { return value; }
@d class C { value: number = 42; }
export const result: number[] = [new C().value];
