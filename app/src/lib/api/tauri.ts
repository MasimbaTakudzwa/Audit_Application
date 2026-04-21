import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return tauriInvoke<T>(cmd, args);
}

export interface HealthStatus {
  app: string;
  version: string;
}

export interface ClientSummary {
  id: string;
  name: string;
  country: string;
  industry: string | null;
  status: string;
}

export interface EngagementSummary {
  id: string;
  name: string;
  client_name: string;
  status: string;
  fiscal_year: string | null;
  created_at: number;
}

export interface LibraryVersion {
  version: string;
  frameworks: string[];
}

export interface CurrentUser {
  signed_in: boolean;
  display_name: string | null;
  firm_name: string | null;
  role: string | null;
}

export interface Session {
  user_id: string;
  firm_id: string;
  display_name: string;
  email: string;
}

export type AuthStatus =
  | { kind: "onboarding_required" }
  | { kind: "sign_in_required" }
  | { kind: "signed_in"; user: Session };

export interface OnboardInput {
  firm_name: string;
  firm_country: string;
  display_name: string;
  email: string;
  password: string;
}

export interface LoginInput {
  email: string;
  password: string;
}

export const api = {
  ping: () => invoke<HealthStatus>("ping"),
  currentUser: () => invoke<CurrentUser>("current_user"),
  authStatus: () => invoke<AuthStatus>("auth_status"),
  onboard: (input: OnboardInput) => invoke<Session>("onboard", { input }),
  login: (input: LoginInput) => invoke<Session>("login", { input }),
  logout: () => invoke<void>("logout"),
  listClients: () => invoke<ClientSummary[]>("list_clients"),
  listEngagements: () => invoke<EngagementSummary[]>("list_engagements"),
  libraryVersion: () => invoke<LibraryVersion>("library_version"),
};
