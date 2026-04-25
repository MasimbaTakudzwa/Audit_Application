import { writable } from "svelte/store";

export type RouteId =
  | "dashboard"
  | "clients"
  | "engagements"
  | "engagement-detail"
  | "working-paper"
  | "library"
  | "settings";

export const currentRoute = writable<RouteId>("dashboard");

// Detail routes carry the owning entity id alongside the route. We keep a
// single slot per detail view rather than a full history stack — sufficient
// while there's only one level of drill-in, and dodges URL routing inside
// Tauri (where there's no real address bar to reconcile with).
export const currentEngagementId = writable<string | null>(null);
export const currentTestId = writable<string | null>(null);

export function openEngagement(id: string) {
  currentEngagementId.set(id);
  currentRoute.set("engagement-detail");
}

export function openWorkingPaper(engagementId: string, testId: string) {
  currentEngagementId.set(engagementId);
  currentTestId.set(testId);
  currentRoute.set("working-paper");
}
