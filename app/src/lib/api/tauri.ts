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

// Engagement "Today" overview — synthesised state returned by
// `engagement_overview`. The frontend renders status counts, risk-coverage
// strip, attention list, and recent activity from a single round-trip
// rather than aggregating across five list endpoints.
export interface EngagementHeader {
  id: string;
  name: string;
  client_name: string;
  status: string;
  fiscal_year: string | null;
  period_start: string | null;
  period_end: string | null;
  library_version_at_start: string;
  created_at: number;
  closed_at: number | null;
  lead_partner_name: string | null;
}

export interface StatusCounts {
  controls_total: number;
  risks_total: number;
  tests_total: number;
  tests_not_started: number;
  tests_in_progress: number;
  tests_in_review: number;
  tests_completed: number;
  results_total: number;
  results_pass: number;
  results_exception: number;
  results_fail: number;
  findings_total: number;
  findings_draft: number;
  findings_issued: number;
  findings_remediated: number;
  findings_closed: number;
  findings_critical: number;
  findings_high: number;
  findings_medium: number;
  findings_low: number;
  findings_observation: number;
  data_imports_total: number;
  evidence_total: number;
}

export interface RiskCoverageEntry {
  risk_id: string;
  risk_code: string;
  risk_title: string;
  inherent_rating: string;
  residual_rating: string | null;
  control_count: number;
  test_count: number;
  tests_with_results: number;
  tests_with_exceptions: number;
  findings_open: number;
  // "uncovered" | "untested" | "tested_clean" | "tested_with_exceptions"
  coverage_state: string;
}

export interface AttentionItem {
  // Stable machine-readable kind. Common values:
  //   "test_in_review" | "exception_no_finding" |
  //   "finding_draft_high_severity" | "risk_no_control" | "control_no_test"
  kind: string;
  // "high" | "medium" | "low" — backend already orders the list.
  priority: string;
  label: string;
  entity_type: string | null;
  entity_id: string | null;
}

export interface RecentActivityEntry {
  at: number;
  actor_name: string | null;
  action: string;
  entity_type: string;
  entity_id: string;
  summary: string | null;
}

export interface EngagementOverview {
  engagement: EngagementHeader;
  status_counts: StatusCounts;
  risk_coverage: RiskCoverageEntry[];
  needs_attention: AttentionItem[];
  recent_activity: RecentActivityEntry[];
}

export interface LibraryVersion {
  version: string;
  frameworks: string[];
}

export interface LibraryRiskSummary {
  id: string;
  code: string;
  title: string;
  default_inherent_rating: string | null;
  applicable_system_types: string[];
}

export interface LibraryControlSummary {
  id: string;
  code: string;
  title: string;
  objective: string;
  control_type: string;
  frequency: string | null;
  applicable_system_types: string[];
  frameworks: string[];
  test_procedure_count: number;
}

export interface LibraryFrameworkMapping {
  framework: string;
  reference: string;
}

export interface LibraryTestProcedureSummary {
  id: string;
  code: string;
  name: string;
  objective: string;
  steps: string[];
  sampling_default: string;
  automation_hint: string;
  evidence_checklist: string[];
}

export interface LibraryControlDetail {
  id: string;
  code: string;
  title: string;
  description: string;
  objective: string;
  control_type: string;
  frequency: string | null;
  applicable_system_types: string[];
  related_risks: LibraryRiskSummary[];
  framework_mappings: LibraryFrameworkMapping[];
  test_procedures: LibraryTestProcedureSummary[];
  library_version: string;
}

export interface AddLibraryControlInput {
  engagement_id: string;
  library_control_id: string;
  system_id: string | null;
}

export interface AddLibraryControlResult {
  engagement_control_id: string;
  engagement_risk_ids: string[];
  test_ids: string[];
}

export interface UploadDataImportInput {
  engagement_id: string;
  system_id: string | null;
  source_kind: string;
  purpose_tag: string;
  filename: string;
  mime_type: string | null;
  content: number[];
}

export interface DataImportSummary {
  id: string;
  filename: string | null;
  source_kind: string;
  purpose_tag: string | null;
  row_count: number | null;
  plaintext_size: number | null;
  imported_at: number;
  imported_by: string | null;
  imported_by_name: string | null;
}

export interface TestSummary {
  id: string;
  engagement_control_id: string;
  control_code: string;
  control_title: string;
  code: string;
  name: string;
  objective: string;
  automation_tier: string;
  status: string;
  latest_result_outcome: string | null;
  latest_result_at: number | null;
  latest_result_evidence_count: number | null;
}

export interface RunMatcherInput {
  test_id: string;
  /**
   * Map of purpose_tag to DataImport.id, pinning a specific import for one
   * or more of the rule's inputs. Normally `null`: the backend picks the
   * newest matching import per purpose tag.
   */
  overrides: Record<string, string> | null;
}

export interface MatcherRunResult {
  test_result_id: string;
  rule: string;
  outcome: string;
  exception_count: number;
  primary_import_id: string;
  primary_import_filename: string | null;
  supporting_import_id: string | null;
  supporting_import_filename: string | null;
  // User access review family
  ad_rows_considered: number | null;
  ad_rows_skipped_disabled: number | null;
  leaver_rows_considered: number | null;
  hr_rows_considered: number | null;
  ad_rows_skipped_unmatchable: number | null;
  ad_rows_skipped_no_last_logon: number | null;
  ad_rows_skipped_unparseable: number | null;
  dormancy_threshold_days: number | null;
  // Change management family
  changes_considered: number | null;
  changes_skipped_standard: number | null;
  changes_skipped_cancelled: number | null;
  changes_skipped_not_deployed: number | null;
  changes_skipped_no_id: number | null;
  changes_skipped_unparseable_dates: number | null;
  // CHG SoD (dev-vs-deploy) — two permission-list inputs, not a change log
  deploy_rows_considered: number | null;
  deploy_rows_skipped_unmatchable: number | null;
  source_rows_considered: number | null;
  source_rows_skipped_unmatchable: number | null;
  intersecting_users: number | null;
  // Backup family
  jobs_considered: number | null;
  jobs_skipped_no_id: number | null;
  jobs_skipped_unknown_status: number | null;
  // IT application controls family
  transactions_considered: number | null;
  transactions_skipped_unparseable: number | null;
  transactions_skipped_zero: number | null;
  digit_rows_evaluated: number | null;
  // Duplicate-transaction detection (ITAC-T-002)
  transactions_skipped_no_key: number | null;
  duplicate_group_count: number | null;
  total_duplicate_rows: number | null;
  // Boundary / threshold analysis (ITAC-T-003)
  thresholds_evaluated: number | null;
  thresholds_flagged: number | null;
  // Periodic recertification (UAM-T-002)
  review_rows_considered: number | null;
  review_rows_skipped_unmatchable: number | null;
  unreviewed_count: number | null;
  unremediated_count: number | null;
  remediation_check_applied: boolean | null;
  remediation_window_days: number | null;
  // Recurring-amount detection (ITAC-T-004)
  transactions_skipped_no_counterparty: number | null;
  transactions_skipped_below_significance: number | null;
  recurring_group_count: number | null;
  total_recurring_rows: number | null;
  recurring_min_amount_cents: number | null;
}

export interface TestResultSummary {
  id: string;
  test_id: string;
  test_code: string;
  test_name: string;
  outcome: string;
  exception_summary: string | null;
  evidence_count: number;
  performed_at: number;
  performed_by_name: string | null;
  population_ref_label: string | null;
  detail_json: string | null;
  notes_blob_id: string | null;
  has_linked_finding: boolean;
}

export interface ElevateFindingInput {
  test_result_id: string;
  title: string | null;
  severity_id: string | null;
}

export interface UpdateFindingInput {
  finding_id: string;
  title: string;
  condition_text: string | null;
  criteria_text: string | null;
  cause_text: string | null;
  effect_text: string | null;
  recommendation_text: string | null;
  severity_id: string;
}

export interface FindingSummary {
  id: string;
  engagement_id: string;
  code: string;
  title: string;
  condition_text: string | null;
  criteria_text: string | null;
  cause_text: string | null;
  effect_text: string | null;
  recommendation_text: string | null;
  severity_id: string | null;
  severity_name: string | null;
  status: string;
  test_id: string | null;
  test_code: string | null;
  engagement_control_id: string | null;
  control_code: string | null;
  identified_at: number;
  identified_by_name: string | null;
  linked_test_result_ids: string[];
}

export interface SeveritySummary {
  id: string;
  name: string;
  sort_order: number;
  description: string | null;
}

export interface EvidenceSummary {
  id: string;
  engagement_id: string;
  title: string;
  description: string | null;
  source: string;
  filename: string | null;
  mime_type: string | null;
  plaintext_size: number | null;
  test_id: string | null;
  test_code: string | null;
  test_result_id: string | null;
  data_import_id: string | null;
  obtained_at: number;
  obtained_from: string | null;
  created_at: number;
  created_by_name: string | null;
  linked_test_ids: string[];
  linked_finding_ids: string[];
}

export interface EvidencePayload {
  id: string;
  filename: string | null;
  mime_type: string | null;
  content: number[];
}

export interface UploadEvidenceInput {
  engagement_id: string;
  title: string;
  description: string | null;
  obtained_from: string | null;
  obtained_at: number | null;
  test_id: string | null;
  finding_id: string | null;
  filename: string;
  mime_type: string | null;
  content: number[];
}

export interface EvidenceLinkInput {
  finding_id: string;
  evidence_id: string;
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
  engagementOverview: (engagementId: string) =>
    invoke<EngagementOverview>("engagement_overview", { engagementId }),
  libraryVersion: () => invoke<LibraryVersion>("library_version"),
  libraryListRisks: () => invoke<LibraryRiskSummary[]>("library_list_risks"),
  libraryListControls: () =>
    invoke<LibraryControlSummary[]>("library_list_controls"),
  libraryGetControl: (id: string) =>
    invoke<LibraryControlDetail>("library_get_control", { id }),
  engagementAddLibraryControl: (input: AddLibraryControlInput) =>
    invoke<AddLibraryControlResult>("engagement_add_library_control", { input }),
  engagementUploadDataImport: (input: UploadDataImportInput) =>
    invoke<DataImportSummary>("engagement_upload_data_import", { input }),
  engagementListDataImports: (engagementId: string) =>
    invoke<DataImportSummary[]>("engagement_list_data_imports", {
      engagementId,
    }),
  engagementListTests: (engagementId: string) =>
    invoke<TestSummary[]>("engagement_list_tests", { engagementId }),
  engagementRunMatcher: (input: RunMatcherInput) =>
    invoke<MatcherRunResult>("engagement_run_matcher", { input }),
  engagementListTestResults: (engagementId: string) =>
    invoke<TestResultSummary[]>("engagement_list_test_results", {
      engagementId,
    }),
  engagementElevateFinding: (input: ElevateFindingInput) =>
    invoke<FindingSummary>("engagement_elevate_finding", { input }),
  engagementUpdateFinding: (input: UpdateFindingInput) =>
    invoke<FindingSummary>("engagement_update_finding", { input }),
  engagementListFindings: (engagementId: string) =>
    invoke<FindingSummary[]>("engagement_list_findings", { engagementId }),
  listFindingSeverities: () =>
    invoke<SeveritySummary[]>("list_finding_severities"),
  engagementListEvidence: (engagementId: string) =>
    invoke<EvidenceSummary[]>("engagement_list_evidence", { engagementId }),
  engagementUploadEvidence: (input: UploadEvidenceInput) =>
    invoke<EvidenceSummary>("engagement_upload_evidence", { input }),
  engagementDownloadEvidence: (evidenceId: string) =>
    invoke<EvidencePayload>("engagement_download_evidence", { evidenceId }),
  findingAttachEvidence: (input: EvidenceLinkInput) =>
    invoke<EvidenceSummary>("finding_attach_evidence", { input }),
  findingDetachEvidence: (input: EvidenceLinkInput) =>
    invoke<void>("finding_detach_evidence", { input }),
  findingListEvidence: (findingId: string) =>
    invoke<EvidenceSummary[]>("finding_list_evidence", { findingId }),
};
