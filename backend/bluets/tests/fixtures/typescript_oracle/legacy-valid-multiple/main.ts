declare function dc(target: any): any;
declare function dm(target: any, key: string, descriptor: any): any;
declare function dp(target: any, key: string): void;
declare function dparam(target: any, key: string | undefined, index: number): void;
@dc @dc class C { @dp @dp f: number = 1; }
export {};
