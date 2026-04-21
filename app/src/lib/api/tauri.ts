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

export interface IndustrySummary {
  id: string;
  name: string;
}

export interface NewClientInput {
  name: string;
  country: string;
  industry_id: string | null;
}

export interface EngagementSummary {
  id: string;
  name: string;
  client_name: string;
  status: string;
  fiscal_year: string | null;
  created_at: number;
}

export interface NewEngagementInput {
  client_id: string;
  name: string;
  fiscal_year_label: string | null;
  period_start: string | null;
  period_end: string | null;
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

export interface UserRecord {
  id: string;
  email: string;
  display_name: string;
  role_id: string;
  role_name: string;
  status: string;
  last_seen_at: number | null;
  created_at: number;
}

export interface RoleRecord {
  id: string;
  name: string;
}

export interface CreateUserInput {
  email: string;
  display_name: string;
  password: string;
  role_id: string;
}

export interface ChangePasswordInput {
  old_password: string;
  new_password: string;
}

export const api = {
  ping: () => invoke<HealthStatus>("ping"),
  currentUser: () => invoke<CurrentUser>("current_user"),
  authStatus: () => invoke<AuthStatus>("auth_status"),
  onboard: (input: OnboardInput) => invoke<Session>("onboard", { input }),
  login: (input: LoginInput) => invoke<Session>("login", { input }),
  logout: () => invoke<void>("logout"),
  resetIdentity: (confirmation: string) =>
    invoke<void>("reset_identity", { confirmation }),
  listUsers: () => invoke<UserRecord[]>("list_users"),
  listRoles: () => invoke<RoleRecord[]>("list_roles"),
  createUser: (input: CreateUserInput) => invoke<UserRecord>("create_user", { input }),
  changePassword: (input: ChangePasswordInput) =>
    invoke<void>("change_password", { input }),
  listClients: () => invoke<ClientSummary[]>("list_clients"),
  listIndustries: () => invoke<IndustrySummary[]>("list_industries"),
  createClient: (input: NewClientInput) =>
    invoke<ClientSummary>("create_client", { input }),
  listEngagements: () => invoke<EngagementSummary[]>("list_engagements"),
  createEngagement: (input: NewEngagementInput) =>
    invoke<EngagementSummary>("create_engagement", { input }),
  libraryVersion: () => invoke<LibraryVersion>("library_version"),
};
