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
interface CardProps { title: string; count?: number; children?: string }
function Card(props: CardProps): JSX.Element { return { tag: props.title }; }
const a = <Card title="t" />;
const b = <Card title="t" count={2}>hello</Card>;
