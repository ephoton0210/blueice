declare function make(n: number): (target: any, key: string) => void;
class C { @make(1) f: number = 1; }
export {};
