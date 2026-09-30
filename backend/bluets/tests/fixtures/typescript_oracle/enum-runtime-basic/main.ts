// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

enum Color { Red, Green = 5, Blue }
enum Label { A = "a", B = "b" }
enum Mixed { N = 1, S = "s", M = 2 }
enum Calc { A = 1 << 2, B = A | 1, C = Calc.B * 2, D = Calc["C"] + A, E = (A | 8) ^ 2, F = -7 % 3, G = ~5 }
enum Quoted { "a-b" = 1, "c d", plain }
const c: Color = Color.Blue;
const n: number = c + 1;
const s: string = Label.A;
console.log(Color.Red, Color.Green, Color.Blue, Color[0], Color[5], Color[6], Color["Green"]);
console.log(Label.A, Label.B, Mixed.N, Mixed.S, Mixed.M, Mixed[1], Mixed[2]);
console.log(Calc.A, Calc.B, Calc.C, Calc.D, Calc.E, Calc.F, Calc.G, Quoted["a-b"], Quoted["c d"], Quoted.plain);
console.log(Object.keys(Color).join(","), JSON.stringify(Label), JSON.stringify(Calc), Object.keys(Quoted).join(","));
console.log(c, n, s, typeof Color, Object.keys(Mixed).join(","));
