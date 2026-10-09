// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let defaults = 0;
export const effects: string[] = [];
function fallback(): number { defaults++; effects.push("default"); return 20; }
export async function calculate(left: number = fallback(), right: number = 22,): Promise<number> {
    effects.push("start");
    try {
        const value = await Promise.resolve(left);
        effects.push("resume");
        await Promise.reject("rejected");
        return value + right;
    } catch (error) {
        effects.push(typeof error);
        return left + right;
    } finally {
        effects.push("finally");
    }
}
export function count(): number { return defaults; }
