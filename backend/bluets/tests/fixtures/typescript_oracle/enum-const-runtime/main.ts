// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const enum Num { A = 1, B, C = 10, Neg = -3, Float = 1.5, Big = 2 ** 40 }
const enum Str { X = "x", Y = "y z", "quoted-key" = "q" }
enum Plain { P, Q = 5 }
class Holder {
    field: number = Num.C;
    static shared: string = Str.X;
    describe(): string { return Str.Y + ":" + Num.B + ":" + Str["quoted-key"]; }
}
function pick(flag: boolean): number { return flag ? Num.A : Num.Neg; }
const fromTemplate: string = `${Num.A}-${Str.X}-${Num.Float}`;
const h = new Holder();
console.log(Num.A, Num.B, Num.C, Num.Neg, Num.Float, Num.Big, Str.X, Str.Y, Str["quoted-key"]);
console.log(h.field, Holder.shared, h.describe(), pick(true), pick(false), fromTemplate);
console.log(Num.A + Num.B * Num.C, Num.Neg + 1, typeof Num.A, typeof Str.X, Plain.Q, Plain[5]);
console.log(Num["B"], Str["X"], (Num.Neg).toString(), Str.X.toUpperCase(), Num.C.toFixed(1));
