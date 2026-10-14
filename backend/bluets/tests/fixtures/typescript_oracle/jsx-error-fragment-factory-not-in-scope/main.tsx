declare namespace JSX {
  interface Element { tag: string }
  interface IntrinsicElements {
    div: { id?: string; hidden?: boolean; onClick?: (n: number) => void; children?: any };
    span: { title: string; children?: any };
    input: { value?: string; disabled?: boolean; kind?: "text" | "number"; children?: any };
  }
  interface IntrinsicAttributes { key?: string }
  interface ElementChildrenAttribute { children: {} }
}
declare function el(tag: any, props: any, ...children: any[]): JSX.Element;
const b = <></>;
