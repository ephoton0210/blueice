// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/** @jsxRuntime classic */
/** @jsx h */
function h(tag: any, props: any): any { return { tag: props.value }; }
declare namespace h { namespace JSX { interface Element { tag: number } interface IntrinsicElements { item: { value: number } } } }
const element = <item value={42} />;
export const result: number[] = [element.tag];
