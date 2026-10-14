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
const n = 4;
const a = <div id={"v" + n} hidden={true} onClick={(k: number) => k + 1} />;
const b = <input kind="number" value="1" disabled />;
