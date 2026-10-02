declare function bad(target: any, key: string): number;
class C { @bad f: number = 1; }
export {};
