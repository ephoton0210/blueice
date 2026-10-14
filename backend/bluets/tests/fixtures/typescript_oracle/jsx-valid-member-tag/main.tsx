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
namespace Ui { export function Item(props: { label: string }): JSX.Element { return { tag: props.label }; } }
const a = <Ui.Item label="l" />;
