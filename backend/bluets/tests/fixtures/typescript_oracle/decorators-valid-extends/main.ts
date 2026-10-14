declare function dec(value: any, context: any): any;
class B {}
@dec class C extends B { @dec m(): void {} }
export {};
