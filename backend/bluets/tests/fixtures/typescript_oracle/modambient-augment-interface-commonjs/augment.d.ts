import type { Item } from "./dep"; export type Marker = number; declare module "./dep" { interface Item { tag: string; } }
