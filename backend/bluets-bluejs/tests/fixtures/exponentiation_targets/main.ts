// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function operand(name: string, value: number): number;
declare function receiver(name: string): any;

export function associated(): number {
    return operand("left", 2) ** operand("middle", 3) ** operand("right", 2);
}

export function grouped(): number {
    return (operand("left", 2) ** operand("middle", 3)) ** operand("right", 2);
}

export function unary(): number {
    return operand("left", 2) ** -operand("right", 2);
}

export function negative(): number {
    return (-operand("left", 2)) ** operand("right", 3);
}

export function updated(): number {
    let value = 2;
    return value++ ** ++value;
}

export function members(): number {
    return receiver("left").read() ** receiver("right").exponent;
}
