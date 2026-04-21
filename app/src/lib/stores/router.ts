import { writable } from "svelte/store";

export type RouteId =
  | "dashboard"
  | "clients"
  | "engagements"
  | "library"
  | "settings";

export const currentRoute = writable<RouteId>("dashboard");
