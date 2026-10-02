declare function dm(target: any, key: string, descriptor: any): any;
class C { @dm get g(): number { return 1; } @dm set g(v: number) {} }
export {};
