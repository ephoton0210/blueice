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
 
}}
const element = <item {...{ value: 41 }} value={42} />;
export const result: number[] = [element.tag];
