declare function make(n: number): (value: any, context: any) => void;
@make("not a number") class C {}
export {};
