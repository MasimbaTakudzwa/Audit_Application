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

export const api = {
  ping: () => invoke<HealthStatus>("ping"),
  currentUser: () => invoke<CurrentUser>("current_user"),
  listClients: () => invoke<ClientSummary[]>("list_clients"),
  listEngagements: () => invoke<EngagementSummary[]>("list_engagements"),
  libraryVersion: () => invoke<LibraryVersion>("library_version"),
};
