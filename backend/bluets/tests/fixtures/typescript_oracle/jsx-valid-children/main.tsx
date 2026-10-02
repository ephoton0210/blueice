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
const label = "n";
const a = <div>text {label} <span title="s" /> more</div>;
const b = <div>{label}</div>;
const c = <div>
  <span title="a" />
  <span title="b" />
</div>;
