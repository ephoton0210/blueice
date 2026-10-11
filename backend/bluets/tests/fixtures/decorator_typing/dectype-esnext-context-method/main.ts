// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(value: any, context: ClassMethodDecoratorContext<C, (n: number) => number>): void {
    const kind: "method" = context.kind;
    const name: string | symbol = context.name;
    const isStatic: boolean = context.static;
    const isPrivate: boolean = context.private;
    const has: (object: C) => boolean = context.access.has;
    const get: (object: C) => (n: number) => number = context.access.get;
    const metadata: DecoratorMetadata = context.metadata;
    context.addInitializer(function(this: C): void {});
}
class C { @d m(n: number): number { return n; } }
export const result: number[] = [new C().m(42)];
