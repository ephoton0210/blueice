declare function dc(target: any): any;
declare function dm(target: any, key: string, descriptor: any): any;
declare function dp(target: any, key: string): void;
declare function dparam(target: any, key: string | undefined, index: number): void;
@dc class C {
  @dp f: number = 1;
  @dp static sf: number = 2;
  @dm m(@dparam a: number): void {}
  @dm static sm(): void {}
  @dm get g(): number { return 1; }
  set g(v: number) {}
  constructor(@dparam private x: number) {}
}
export {};
