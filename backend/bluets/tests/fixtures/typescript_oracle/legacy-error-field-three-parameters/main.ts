declare function three(target: any, key: string, descriptor: any): void;
class C { @three f: number = 1; }
export {};
