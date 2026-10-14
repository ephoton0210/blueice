declare function bad(value: any, context: any): number;
class C { @bad f: number = 1; }
export {};
