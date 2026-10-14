declare function dec(value: any, context: any): any;
class C {
  @dec m(): void {}
  @dec static sm(): void {}
  @dec get g(): number { return 1; }
  @dec set g(v: number) {}
  @dec f: number = 1;
  @dec static sf: number = 2;
  @dec accessor a: number = 3;
  @dec #p: number = 4;
}
export {};
