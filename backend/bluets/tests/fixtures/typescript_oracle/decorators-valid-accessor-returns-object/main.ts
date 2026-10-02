declare function acc(value: any, context: any): { init: (v: any) => any };
class C { @acc accessor a: number = 1; }
export {};
