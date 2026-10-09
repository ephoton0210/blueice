import type { NewItem } from "./dep"; import type { Marker } from "./augment"; export type Witness = Marker; export const answer: NewItem = { value: 42 }; console.log(answer.value);
