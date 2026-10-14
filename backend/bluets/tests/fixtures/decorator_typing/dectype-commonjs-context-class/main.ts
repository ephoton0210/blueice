// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function d(value: any, context: ClassDecoratorContext<typeof C>): void {
    const kind: "class" = context.kind;
    const name: string | undefined = context.name;
    const metadata: DecoratorMetadata = context.metadata;
    context.addInitializer(function(this: typeof C): void {});
}
@d class C { value: number = 42; }
export const result: number[] = [new C().value];
