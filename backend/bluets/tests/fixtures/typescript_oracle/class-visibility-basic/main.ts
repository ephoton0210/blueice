// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Account {
    private balance: number = 0;
    protected owner: string = "ada";
    public label: string = "acct";
    private static count: number = 0;
    protected static prefix: string = "A-";
    private bump(): number { this.balance = this.balance + 1; return this.balance; }
    protected who(): string { return this.owner; }
    deposit(other: Account): number {
        Account.count = Account.count + 1;
        return this.bump() + other.balance + other.bump();
    }
    static made(): number { return Account.count; }
}
class Savings extends Account {
    describe(): string { return this.owner + this.who() + Savings.prefix; }
    audit(other: Savings): string { return other.owner + other.who(); }
}
const s = new Savings();
const text: string = s.describe() + s.label;
const total: number = s.deposit(new Account()) + Account.made();
