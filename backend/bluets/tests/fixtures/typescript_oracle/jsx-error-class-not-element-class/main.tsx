declare namespace JSX {
  interface Element { tag: string }
  interface IntrinsicElements {
    div: { id?: string; hidden?: boolean; onClick?: (n: number) => void; children?: any };
    span: { title: string; children?: any };
    input: { value?: string; disabled?: boolean; kind?: "text" | "number"; children?: any };
  }
  interface IntrinsicAttributes { key?: string }
  interface ElementClass { render(): Element }
  interface ElementAttributesProperty { props: {} }
  interface ElementChildrenAttribute { children: {} }
}
class NotComp { props: { a: number } = { a: 1 }; }
const a = <NotComp a={1} />;
