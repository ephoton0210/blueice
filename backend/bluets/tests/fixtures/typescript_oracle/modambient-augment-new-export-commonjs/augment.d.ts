import type { Existing } from "./dep"; export type Marker = number; declare module "./dep" { interface NewItem { value: number; } }
