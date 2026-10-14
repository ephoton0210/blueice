declare function bad(target: any, key: string | undefined, index: number): string;
class C { m(@bad a: number): void {} }
export {};
