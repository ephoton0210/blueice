declare function wrap(value: any, context: any): () => any;
class C { @wrap m(): number { return 1; } @wrap get g(): number { return 1; } }
export {};
