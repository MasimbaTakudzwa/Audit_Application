import { writable } from "svelte/store";
import { api, type Session, type AuthStatus } from "../api/tauri";

type AuthView =
  | { state: "loading" }
  | { state: "onboarding" }
  | { state: "sign_in" }
  | { state: "signed_in"; session: Session };

export const authView = writable<AuthView>({ state: "loading" });

function applyStatus(status: AuthStatus) {
  switch (status.kind) {
    case "onboarding_required":
      authView.set({ state: "onboarding" });
      return;
    case "sign_in_required":
      authView.set({ state: "sign_in" });
      return;
    case "signed_in":
      authView.set({ state: "signed_in", session: status.user });
      return;
  }
}

export async function refreshAuth() {
  const status = await api.authStatus();
  applyStatus(status);
}

export async function onboard(input: {
  firm_name: string;
  firm_country: string;
  display_name: string;
  email: string;
  password: string;
}) {
  const session = await api.onboard(input);
  authView.set({ state: "signed_in", session });
}

export async function login(input: { email: string; password: string }) {
  const session = await api.login(input);
  authView.set({ state: "signed_in", session });
}

export async function logout() {
  await api.logout();
  authView.set({ state: "sign_in" });
}
