declare function bad(value: any, context: any): () => void;
class C { @bad accessor a: number = 1; }
export {};
