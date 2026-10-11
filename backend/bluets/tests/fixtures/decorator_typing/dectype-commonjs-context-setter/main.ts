// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(value: any, context: ClassSetterDecoratorContext<C, number>): void {
    const kind: "setter" = context.kind;
    const name: string | symbol = context.name;
    const isStatic: boolean = context.static;
    const isPrivate: boolean = context.private;
    const has: (object: C) => boolean = context.access.has;
    const set: (object: C, value: number) => void = context.access.set;
    const metadata: DecoratorMetadata = context.metadata;
    context.addInitializer(function(this: C): void {});
}
class C { @d set value(n: number) {} answer: number = 42; }
export const result: number[] = [new C().answer];
