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
interface Props { label: string; size?: number }
class Panel {
  props: Props = { label: "l" };
  render(): JSX.Element { return { tag: this.props.label }; }
}
const a = <Panel label={1} />;
