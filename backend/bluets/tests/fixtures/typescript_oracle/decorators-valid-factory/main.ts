declare function make(n: number): (value: any, context: any) => void;
@make(1) class C { @make(2) m(): void {} }
export {};
