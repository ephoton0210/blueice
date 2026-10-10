// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function h(tag: any, props: any, ...children: any[]): any { return { tag: props && props.value !== undefined ? props.value : 42 }; }
declare namespace h { namespace JSX {
 interface Element { tag: number }
 interface IntrinsicElements { item: { value: number; children?: number; 'ns:value'?: number } }
 interface IntrinsicAttributes { key?: string }
 interface ElementChildrenAttribute { children: {} }
 interface ElementClass { render(): Element }
 interface ElementAttributesProperty { props: {} }
 type LibraryManagedAttributes<C, P> = C extends { defaultProps: infer D } ? Omit<P, keyof D> & Partial<Pick<P, Extract<keyof P, keyof D>>> : P;
}}
function Component(props: { value: number }): h.JSX.Element { return { tag: props.value }; }
namespace Component { export const defaultProps = { value: 42 }; }
const element = <Component />;
export const result: number[] = [element.tag];
