// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export class Vault {
    private balance: number = 0;
    protected owner: string = "ada";
    public label: string = "v";
    private readonly id: number = 1;
    protected readonly kind = "vault";
    private static count: number = 0;
    protected static prefix: string = "V-";
    private bump(step: number): number { this.balance = this.balance + step; return this.balance; }
    protected who(): string { return this.owner; }
    public open(): number { return this.bump(1); }
    private static reset(): number { return Vault.count; }
    protected static tag(): string { return Vault.prefix; }
    private tail?: string;
    protected constructor(seed: number) { this.balance = seed; }
    static make(): Vault { return new Vault(1); }
}
export class Sealed {
    private constructor() {}
    static create(): Sealed { return new Sealed(); }
}
