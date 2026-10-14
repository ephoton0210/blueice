// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class Vault {
    private balance: number = 0;
    protected owner: string = "ada";
    public label: string = "v";
    private static count: number = 0;
    protected static prefix: string = "V-";
    private bump(step: number): number { this.balance = this.balance + step; return this.balance; }
    protected who(): string { return this.owner + Vault.prefix; }
    public open(): number { Vault.count = Vault.count + 1; return this.bump(1) + Vault.count; }
    protected constructor(seed: number) { this.balance = seed; }
    static make(seed: number): Vault { return new Vault(seed); }
}
class Branch extends Vault {
    constructor() { super(10); }
    describe(): string { return this.who() + this.label + super.who(); }
}
const branch = new Branch();
console.log(branch.open(), branch.describe(), Vault.make(5).open());
