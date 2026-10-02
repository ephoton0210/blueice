declare function opt(target: any, key?: string, descriptor?: any): void;
class C { @opt m(): void {} @opt f: number = 1; }
@opt class D {}
export {};
