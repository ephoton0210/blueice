declare function dparam(target: any, key: string | undefined, index: number): void;
class C { get g(): number { return 1; } set g(@dparam v: number) {} }
export {};
