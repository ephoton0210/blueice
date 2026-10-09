import type { Marker } from "./augment"; import type { Item } from "./dep"; export type Witness = Marker; export const answer: Item = { value: 42, tag: "ok" }; console.log(answer.value);
