export declare namespace JSX {
  interface Element { tag: string }
  interface IntrinsicElements { div: { id?: string; children?: any } }
}
export function createElement(tag: any, props: any, ...children: any[]): JSX.Element { return { tag: "x" }; }
export function Fragment(props: any): JSX.Element { return { tag: "frag" }; }
