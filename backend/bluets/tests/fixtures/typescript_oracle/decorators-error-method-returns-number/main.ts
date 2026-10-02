declare function bad(value: any, context: any): number;
class C { @bad m(): void {} }
export {};
