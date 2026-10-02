declare function dec(value: any, context: any): any;
const holder = { dec: dec };
@holder.dec class C { @holder.dec m(): void {} }
export {};
