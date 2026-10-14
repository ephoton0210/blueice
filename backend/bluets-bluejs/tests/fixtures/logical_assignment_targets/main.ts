// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare let provided: number | null | undefined;
declare function effect(): number;

export function operate(): unknown {
    let left = 0;
    let right = 1;
    left ||= effect();
    right &&= effect();
    return [left, right];
}

export function nullish(): unknown { return provided ??= effect(); }

export function conditional(flag: boolean): unknown {
    let left = 0;
    let right = 0;
    left ||= flag ? right ||= effect() : 7;
    return [left, right];
}

export function collision(): unknown {
    let __blueice_target_value_0 = 17;
    let value: number | undefined;
    value ??= 42;
    return [__blueice_target_value_0, value];
}

export async function suspended(): Promise<unknown> {
    let value: number | undefined;
    value ??= await Promise.resolve(42);
    return value;
}
