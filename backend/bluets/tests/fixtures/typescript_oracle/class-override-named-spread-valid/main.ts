// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
type Pair = [number, string];
type Args = [boolean, ...Pair];
class FirstBase { method(...args: Args): void {} }
class FirstGood extends FirstBase { method(...args: [boolean, number, string]): void {} }
class SecondBase { method(...args: [boolean, number, string]): void {} }
class SecondGood extends SecondBase { method(...args: [boolean, ...Pair]): void {} }
class StaticBase { static method(...args: Args): void {} }
class StaticGood extends StaticBase { static method(...args: [boolean, number, string]): void {} }
type Prefix<T extends unknown[]> = [boolean, ...T];
class GenericBase { method(...args: Prefix<[number, string]>): void {} }
class GenericGood extends GenericBase { method(...args: [boolean, number, string]): void {} }
type Tail = string[];
type VariableArgs = [number, ...Tail, boolean];
class VariableBase { method(...args: VariableArgs): void {} }
class VariableGood extends VariableBase { method(...args: [number, ...string[], boolean]): void {} }
