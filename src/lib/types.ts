// Types that mirror what the Rust side sends (src-tauri/src/domain, service, scanner/guard).

export type InstitutionKind = "elderly_home" | "children_home" | "other";
export type DependencyLevel = "low" | "medium" | "high" | "total";

export interface InstitutionInput {
  name: string;
  kind: InstitutionKind;
  mission: string | null;
  legal_rfc: string | null;
  contact_phone: string | null;
  contact_email: string | null;
  legal_rep_name: string | null;
  /** Where it is and what it is, legally (ADR-031). */
  state: string | null;
  municipality: string | null;
  founded_year: number | null;
  legal_form: string | null;
  authorized_donee: string | null;
  cluni: string | null;
  /** Whom and how it serves (ADR-033); left out when a window does not edit it, so what is saved stays. */
  attention?: Attention | null;
  /** The data kept as fields of the catalog (ADR-033), by field id: legal name, address, fiscal data… */
  details?: Record<string, FormValue> | null;
}
/** The attention profile: codes of `core::institution::catalog`. */
export interface Attention {
  populations: string[];
  sex_served: string | null;
  modalities: string[];
  care_areas: string[];
}

// Forms described as data (ADR-033, common/forms.rs): the screen draws them, Rust validates and saves them.
export type FieldKind = "text" | "long_text" | "number" | "money" | "year" | "date" | "select" | "multi_select" | "yes_no" | "email" | "phone";
export type Condition = { when: "always" } | { when: "filled"; field: string } | { when: "any_of"; field: string; values: string[] };
export interface FieldSpec {
  id: string;
  kind: FieldKind;
  options: string[];
  required: boolean;
  /** indispensable (ADR-042): the first start asks it */
  essential: boolean;
  applies_when: Condition;
  sensitivity: "public" | "internal" | "institutional_private" | "personal";
  ai: "as_is" | "aggregate_only" | "never";
  used_by: string[];
  min: number | null;
  max: number | null;
}
export interface SectionSpec {
  id: string;
  columns: number;
  fields: FieldSpec[];
}
export interface FormSpec {
  id: string;
  sections: SectionSpec[];
}
export type FormValue = string | number | boolean | string[];
export type FormValues = Record<string, FormValue>;
// The institution at a glance (ADR-033, core/overview.rs): composed in Rust, the screen only draws it.
/** A window of «Mi institución» (ADR-042: what the modules lack is asked from Inicio). */
export type Place = "institution" | "contact" | "legal" | "board" | "capacity";
export interface Figure {
  value: number;
  /** the quick figure of the first start, while the module has no records */
  approx: boolean;
}
export interface InstitutionOverview {
  /** what `people` counts: today where people stay, the last year where they come and go (ERP-2) */
  people_period: "today" | "last_year";
  people: Figure;
  capacity: number | null;
  occupied_percent: number | null;
  vacant: number | null;
  staff: Figure;
  spaces: number;
  balance_annual_mxn: number | null;
  completion: { gaps: { code: string; place: Place }[]; percent: number };
  /** the suggestions of the AI waiting in each window (IA4) */
  suggestions: { place: Place; count: number }[];
}
export interface FormView {
  spec: FormSpec;
  values: FormValues;
  /** what other windows saved that decides a field here (how it serves, for the capacity): read, never saved */
  context: FormValues;
  /** Where each saved datum comes from (ADR-033 §4): `user`, `document`, `ai_assumption` or `computed`. */
  origins: Record<string, FieldOrigin>;
  missing: string[];
  /** the suggestions of the AI waiting in this window (IA4) */
  proposals: Proposal[];
}
/** A suggestion of the AI for a field (ADR-034 §2): nothing is saved until the person uses it and saves. */
export interface Proposal {
  id: string;
  form_id: string;
  field_id: string;
  value: FormValue;
  origin: "ai_assumption" | "document";
  /** the words of the person it was taken from */
  said: string | null;
  /** the document it was read from and its page, when it came from a document (IA6) */
  document: string | null;
  page: number | null;
}
export interface FieldOrigin {
  origin: "user" | "document" | "ai_assumption" | "computed";
  source_ref: string | null;
  confirmed_at: string | null;
}
// The board and the bank accounts of the institution (ADR-033 §3, core/protected): never to the AI.
export type ListKind = "board_member" | "bank_account";
// The record of the institution (ADR-033 §5, core/records): what it must have, with its state.
export type RecordStatus = "have" | "in_progress" | "missing" | "not_applicable";
export interface RecordFile {
  name: string;
  /** false: a picture with no text that could be read */
  readable: boolean;
  /** its text was read from a picture (OCR, ADR-035) */
  from_picture: boolean;
  /** the pages read with doubts */
  doubtful: number[];
}
export interface InstitutionRecord {
  /** null while nothing was written of it */
  id: string | null;
  /** a type of the catalog, or `other` */
  type: string;
  custom_name: string | null;
  status: RecordStatus;
  issued_on: string | null;
  valid_until: string | null;
  validity: "valid" | "expiring" | "expired" | null;
  days_left: number | null;
  file: RecordFile | null;
  origin: string | null;
  confirmed_at: string | null;
}
export interface Expediente {
  records: InstitutionRecord[];
  summary: { expected: number; have: number; in_progress: number; missing: number; expiring: number; expired: number };
}
export interface RecordForm {
  spec: FormSpec;
  values: FormValues;
  missing: string[];
  file: RecordFile | null;
  /** the data of its window read from its file, by field: the page, and whether a rule worked it out (ADR-038) */
  read: Record<string, { page: number | null; computed: boolean }>;
}
export type RecordOutcome =
  | { status: "saved"; id: string; expediente: Expediente }
  | { status: "invalid"; issues: ProfileIssue[] }
  | { status: "quarantine"; report: QuarantineReport };
/** How a reading in the background started (ADR-038): `record-read` says how it ended, with the same `key`. */
export type AttachStart = { status: "reading"; key: string } | { status: "already_reading"; key: string };
/** How a reading in the background ended: what came of the file, or the error. */
export interface RecordRead {
  key: string;
  kind: string;
  outcome?: AttachOutcome;
  error?: { code: string; message: string };
}
export type AttachOutcome =
  /**
   * `covered`: data of people the scanner covered (it never refuses nor asks, 2026-10-13); `filled`: fields of its own
   * window written from the file; `suggested`: suggestions left in «Mi institución»
   */
  | {
      status: "attached";
      id: string;
      readable: boolean;
      from_picture: boolean;
      doubtful: number[];
      covered: number;
      filled: string[];
      suggested: number;
      expediente: Expediente;
    }
  /**
   * Not kept (ADR-042 §4): Rust is not sure the file is the document it was put on, and it waits for the person.
   * `looks_like`: the document its words say, if any; `there_has_file`: that one already has a file, which it replaces.
   */
  | { status: "ask"; looks_like: string | null; file_name: string; there_has_file: boolean }
  | { status: "unreadable"; reason: string };
/** What the person answers about a file that waits (ADR-042 §4). */
export type PlaceAnswer = "here" | "there" | "cancel";
/** How an answer started: kept in the background on the document of `key` (`record-read` says how it ended), or not at all. */
export type PlaceStart = { status: "reading"; key: string; kind: string } | { status: "already_reading"; key: string } | { status: "cancelled" };
export interface BoardMember {
  id: string;
  name: string;
  /** a code of `forms.fields["institution.board.role"]` */
  role: string;
  role_other: string | null;
  since: string | null;
  active: boolean;
  origin: FieldOrigin["origin"];
  confirmed_at: string | null;
}
export interface BankAccount {
  id: string;
  /** the CLABE covered: only its last four digits show */
  masked: string;
  bank: string | null;
  purpose: string[];
  in_documents: boolean;
  active: boolean;
  origin: FieldOrigin["origin"];
  confirmed_at: string | null;
}
export interface InstitutionLists {
  board: BoardMember[];
  accounts: BankAccount[];
}
export interface ListForm {
  spec: FormSpec;
  values: FormValues;
  missing: string[];
  /** the CLABE of a saved account, covered (the form no longer asks it) */
  masked: string | null;
}
export type ListOutcome = { status: "saved"; lists: InstitutionLists } | { status: "invalid"; issues: ProfileIssue[] };
/** A place of the program a button may take the person to, with its name (core/places.rs). */
export interface AppPlace {
  id: string;
  label: string;
}
// The manual of the program (ADR-034, core/manual): in the program, with no AI and no internet.
export interface ManualEntry {
  id: string;
  title: string;
  paragraphs: string[];
  /** what uses the field (codes of `used_by` in the catalog); empty for a screen */
  used_by: string[];
}
export interface ManualHit {
  id: string;
  title: string;
  snippet: string;
}
export interface PopulationGroupInput {
  label: string;
  age_min: number | null;
  age_max: number | null;
  count: number;
  dependency_level: DependencyLevel | null;
  notes: string | null;
  paying_count: number | null;
  monthly_fee_mxn: number | null;
}
export type ContractKind = "permanent" | "temporary" | "fees";
export interface StaffGroupInput {
  role: string;
  count: number;
  shift: string | null;
  paid: boolean;
  monthly_salary_mxn: number | null;
  contract: ContractKind | null;
  start_year: number | null;
  notes: string | null;
}
/** Whether an amount is written per month or per year; Rust turns it into a year (ADR-026). */
export type Period = "monthly" | "annual";
export type IncomeKind = "fee_estimate" | "recurring_donor" | "occasional_donation" | "project_grant" | "other";
export interface IncomeSourceInput {
  label: string;
  kind: IncomeKind;
  amount_mxn: number | null;
  period: Period;
}
export interface ExpenseItemInput {
  label: string;
  amount_mxn: number | null;
  period: Period;
}
export interface ProfileInput {
  institution: InstitutionInput;
  capacity_total: number | null;
  notes: string | null;
  /** Quick figures while the records are not in the modules yet (ADR-031). */
  served_estimate: number | null;
  staff_paid_estimate: number | null;
  staff_volunteer_estimate: number | null;
  population: PopulationGroupInput[];
  staff: StaffGroupInput[];
}

/** Everything the person writes about the money; it lives in the finance module (ADR-032). */
export interface FinanceInput {
  /** «Gasto anual aproximado»: the quick way to start; once the list has a line, the list is the total. */
  annual_budget_mxn: number | null;
  income: IncomeSourceInput[];
  expenses: ExpenseItemInput[];
}

export interface ProfileIssue {
  code: string;
  field: string;
  blocking: boolean;
}
export interface ProfileTotals {
  population: number;
  staff_paid: number;
  staff_volunteer: number;
  payroll_monthly_mxn: number;
  payroll_annual_mxn: number;
  payroll_benefits_annual_mxn: number;
  payroll_cost_annual_mxn: number;
  benefits_assumed: number;
  /** Contributions to the congregation and social-service grants, in a year (not payroll; ADR-027). */
  staff_support_annual_mxn: number;
  /** What outside companies bill for their staff, in a year (not payroll; ADR-027). */
  external_staff_annual_mxn: number;
  fee_payers: number;
  fees_monthly_mxn: number;
  fees_annual_mxn: number;
}

export interface ProfileView {
  institution_id: string;
  version: number;
  confirmed_at: string | null;
  is_draft: boolean;
  input: ProfileInput;
  totals: ProfileTotals;
  issues: ProfileIssue[];
}

/** The money as the finance module shows it: what was written, the sums made by Rust and the heads-ups. */
export interface FinanceView {
  input: FinanceInput;
  finances: Finances;
  issues: ProfileIssue[];
}

/** One line of money, as Rust computed it. `index` points into `FinanceInput.income` / `.expenses`; `null` is a line
 * the app computes from the roster (`beneficiary_fees`, `payroll`) and nobody edits. */
export interface FinanceLine {
  label: string;
  kind: IncomeKind | "beneficiary_fees" | "expense" | "payroll" | "staff_support" | "external_staff";
  index: number | null;
  annual_mxn: number | null;
  counted: boolean;
}
export type ExpenseBasis = "list" | "estimate" | "unknown";
export interface Finances {
  income: FinanceLine[];
  income_by_kind: { kind: FinanceLine["kind"]; annual_mxn: number }[];
  income_fixed_annual_mxn: number;
  income_variable_annual_mxn: number;
  income_annual_mxn: number;
  income_known: boolean;
  expenses: FinanceLine[];
  expenses_basis: ExpenseBasis;
  expenses_annual_mxn: number | null;
  balance_annual_mxn: number | null;
}

export type Decision = "redact" | "not_personal";

export interface FieldReport {
  path: string;
  counts: Record<string, number>;
  blocking: boolean;
  redacted_preview: string;
}
export interface QuarantineReport {
  fields: FieldReport[];
  counts: Record<string, number>;
  has_blocking: boolean;
}

export type SaveProfileOutcome =
  | { status: "saved"; profile: ProfileView }
  | { status: "quarantine"; report: QuarantineReport }
  | { status: "invalid"; issues: ProfileIssue[] };

export type FinanceOutcome =
  | { status: "saved"; finance: FinanceView }
  | { status: "quarantine"; report: QuarantineReport }
  | { status: "invalid"; issues: ProfileIssue[] };

export interface DocumentSummary {
  id: string;
  kind: string;
  display_name: string;
  mime: string;
  data_level: string;
  redactions_count: number;
  chunks: number;
  created_at: string;
  /** how many pages were read from a picture (OCR, ADR-035), and which of them with doubts */
  picture_pages: number;
  doubtful_pages: number[];
  /** the class the person chose and what the document is, a kind of the record (IA6), with who said it */
  category: "legal" | "financial" | "programs" | null;
  doc_type: string | null;
  doc_type_origin: "computed" | "user" | null;
  /** the drawer it belongs to (ADR-036), and the document of the Expediente it is the file of */
  drawer: "record" | "projects" | null;
  record_id: string | null;
  record_type: string | null;
  record_name: string | null;
}
/** A piece of a document that answers a search (IA5, core/archive/search.rs). */
export interface Passage {
  chunk: number;
  document_id: string;
  document: string;
  page: number | null;
  /** the words around what was looked for */
  snippet: string;
}
/** What a search in the documents found: by words. */
export interface Found {
  passages: Passage[];
}

export interface DeleteSummary {
  chunks: number;
  derived_rows: number;
}

export interface AppError {
  code: string;
  message: string;
}

// ---------------------------------------------------------------- AI, projects, diagnosis

export type StageName =
  | "PROFILE"
  | "DIAGNOSIS"
  | "PRIORITIZATION"
  | "CALL_SELECTION"
  | "DRAFTING"
  | "REVIEW"
  | "READY";

export const STAGES: StageName[] = [
  "PROFILE",
  "CALL_SELECTION",
  "DIAGNOSIS",
  "PRIORITIZATION",
  "DRAFTING",
  "REVIEW",
  "READY",
];

export type AiProvider = "gemini" | "anthropic";

export interface AiStatusView {
  provider: AiProvider;
  /** A key is saved for the active provider. */
  has_key: boolean;
  keys: { gemini: boolean; anthropic: boolean };
  model_light: string;
  model_strong: string;
  spent_mxn: number;
  cap_mxn: number;
  percent: number;
  near_cap: boolean;
  paused: boolean;
}


export interface AiModelsView {
  provider: AiProvider;
  light: string;
  strong: string;
  /** Thinking depth of the strong tier; empty = the service's default. */
  effort: string;
  known: string[];
  strong_chain: string[];
}

export type AiStatus =
  | "used"
  | "not_configured"
  | "offline"
  | "busy"
  | "budget_exhausted"
  | "quota_reached"
  | "key_rejected"
  | "unavailable"
  | "skipped";

export interface ModelCheck {
  model: string;
  exists: boolean;
  can_generate: boolean;
}

export interface AiCheckView {
  ok: boolean;
  models: ModelCheck[];
  /** Why it could not be checked (same keys as `AiStatus`). */
  problem: AiStatus | null;
}

export interface RateLimit {
  per_minute: number;
  tokens_per_minute: number;
  per_day: number;
}

export interface ModelUsage {
  provider: string;
  model: string;
  /** Which job the active provider gives this model, if any. */
  tier: "light" | "strong" | null;
  /** It is a backup of that job, not the main model. */
  is_fallback: boolean;
  calls_last_minute: number;
  calls_last_day: number;
  tokens_last_minute: number;
  limit: RateLimit | null;
  calls_total: number;
  failed_total: number;
  input_tokens: number;
  output_tokens: number;
  thought_tokens: number;
  cached_tokens: number;
  avg_latency_ms: number | null;
  p95_latency_ms: number | null;
  cost_mxn: number;
}

export interface TaskUsage {
  task: string;
  calls: number;
  avg_latency_ms: number | null;
  avg_input_tokens: number;
  avg_output_tokens: number;
  avg_thought_tokens: number;
  cost_mxn: number;
}

export interface RecentCall {
  at: string;
  task: string;
  model: string;
  latency_ms: number | null;
  input_tokens: number;
  output_tokens: number;
  thought_tokens: number;
  ok: boolean;
  error_kind: string | null;
}

export interface UsageReport {
  provider: AiProvider;
  month_spend_mxn: number;
  cap_mxn: number;
  models: ModelUsage[];
  tasks: TaskUsage[];
  recent: RecentCall[];
}

export interface ProjectRow {
  id: string;
  institution_id: string;
  profile_id: string;
  title: string;
  initial_request: string | null;
  stage: StageName;
  needs_review: boolean;
  created_at: string;
  /** How it began: from a call, or from an everyday need of the institution (switched off for now). */
  kind: "call" | "internal";
  /** The reading of the call this project was born from. */
  call_reading_id: string | null;
  /** The color of its folder; `null` until the person picks one. */
  color: ProjectColor | null;
  /** Who gives the support; `null` if it was not said. */
  donor_kind: DonorKind | null;
}
export type DonorKind = "institutional" | "private" | "individual";
export type ProjectColor = "blue" | "violet" | "teal" | "green" | "amber" | "orange" | "pink" | "red";

export interface SummaryJson {
  problem_statement: string;
  affected: { group: string; count: number | null; description: string };
  current_consequences: string[];
  root_causes: string[];
  reframed_need: string;
  alternatives: { title: string; pros: string[]; cons: string[] }[];
  suggested_indicators: string[];
  open_questions: string[];
}

export interface StoredSummary {
  summary: SummaryJson;
  origin: string;
  confirmed_at: string | null;
}

export type ConversationPhase = "needs_opening" | "awaiting_answer" | "awaiting_ai" | "root_proposed" | "closed";
export type TurnKind = "opening" | "why" | "root_proposal" | "root_reply";
export interface TurnView {
  turn: number;
  role: "assistant" | "person";
  kind: TurnKind;
  /** Which «why» the assistant asks or the person answers. */
  level: number | null;
  text: string;
  /** Quick replies the assistant offered. */
  options: string[];
}
/** The guided conversation of the diagnosis and, once it ends, the summary (ADR-017). */
export interface ConversationView {
  project: ProjectRow;
  call: { name: string; funder: string | null; year: number | null } | null;
  turns: TurnView[];
  phase: ConversationPhase;
  /** The «why» being asked or answered now (0 before the first). */
  why_level: number;
  max_whys: number;
  /** How well the idea fits what the call funds, as judged when the person answered the opening. */
  fit: { fit: "fits" | "partial" | "mismatch"; note: string } | null;
  root: { text: string; confirmed: boolean } | null;
  summary: StoredSummary | null;
  unsupported_figures: string[];
  /** Started with the seven fixed questions of the earlier method: it has no conversation. */
  legacy: boolean;
}

export type CreateProjectOutcome =
  | { status: "created"; project: ProjectRow }
  | { status: "quarantine"; report: QuarantineReport };


export interface SummaryEdit {
  problem_statement: string;
  reframed_need: string;
  affected_description: string;
  current_consequences: string[];
  root_causes: string[];
  suggested_indicators: string[];
  open_questions: string[];
}

export type EditOutcome =
  | { status: "saved"; view: ConversationView }
  | { status: "quarantine"; report: QuarantineReport };

export interface Scores {
  beneficiaries: number;
  severity: number;
  mission: number;
  feasibility: number;
  sustainability: number;
}

export interface NeedRow {
  id: string;
  title: string;
  description: string | null;
  scores: Scores | null;
  total_score: number | null;
  selected: boolean;
  origin: string;
  confirmed: boolean;
}

export interface NeedsView {
  project: ProjectRow;
  needs: NeedRow[];
  ranking: string[];
  beneficiaries_suggestion: number | null;
}

export type AddNeedOutcome =
  | { status: "saved"; view: NeedsView }
  | { status: "quarantine"; report: QuarantineReport };

// ---------------------------------------------------------------- drafting, review and the guide (ADR-018)

export interface Sourced<T> {
  value: T;
  page: number | null;
  file: string | null;
}
export interface CallLine {
  text: string;
  applies_to: string | null;
  page: number | null;
  file: string | null;
}
/** What the call asks of a project, read from its confirmed reading. What it does not say is null or empty. */
export interface CallRequirements {
  max_amount_mxn: Sourced<number> | null;
  min_amount_mxn: Sourced<number> | null;
  foreign_currency: boolean;
  cofunding_percent: Sourced<number> | null;
  admin_cap_percent: Sourced<number> | null;
  max_duration_months: Sourced<number> | null;
  closing_date: Sourced<string> | null;
  required_docs: CallLine[];
  conditional_docs: CallLine[];
  optional_docs: CallLine[];
  formats: CallLine[];
  evaluation_criteria: CallLine[];
  project_requirements: CallLine[];
  fundable: CallLine[];
  not_fundable: CallLine[];
  indicators: CallLine[];
  how_to_deliver: CallLine[];
  contact: CallLine[];
}

export type SectionKind = "data" | "text" | "budget" | "schedule";
export type SectionStatus = "empty" | "draft_ai" | "draft_user" | "confirmed" | "needs_review";
export interface SectionView {
  key: string;
  title: string;
  guidance: string;
  kind: SectionKind;
  required: boolean;
  source: "base" | "call";
  content: string;
  status: SectionStatus;
  /** What the AI says is missing in its text. */
  open_points: string[];
  /** Numbers in the AI's text that nobody gave. */
  unsupported_figures: string[];
  /** What the section asks, in plain words, as the assistant explained it when the drafting began. */
  plain: string | null;
}

export type Funder = "requested" | "institution" | "other";
export interface BudgetItemView {
  id: string;
  category: string;
  description: string;
  quantity: number;
  unit: string | null;
  unit_price_mxn: number;
  vat_included: boolean;
  funded_by: Funder;
  administrative: boolean;
  /** `ai_assumption` while it is the assistant's proposal; `user` once the person touched it. */
  origin: string;
  line: { subtotal: number; vat: number; total: number };
}
export interface BudgetTotals {
  subtotal: number;
  vat: number;
  total: number;
  requested: number;
  institution: number;
  other: number;
  administrative_requested: number;
  counterpart_percent: number;
  administrative_percent: number;
}
export interface ActivityView {
  id: string;
  title: string;
  start_month: number;
  end_month: number;
  origin: string;
}
export interface DraftingView {
  project: ProjectRow;
  asks_for_proposal: boolean;
  asks_confirmed: boolean;
  requirements: CallRequirements;
  sections: SectionView[];
  budget: { items: BudgetItemView[]; totals: BudgetTotals; confirmed: boolean; missing_prices: number };
  schedule: { activities: ActivityView[]; duration_months: number; confirmed: boolean };
  objective: string | null;
  /** The assistant already prepared the draft (explanations, budget lines and schedule). */
  plan_ready: boolean;
}
export interface BudgetItemInput {
  id: string | null;
  category: string;
  description: string;
  quantity: number;
  unit: string | null;
  unit_price_mxn: number;
  vat_included: boolean;
  funded_by: Funder;
  administrative: boolean;
}
export type DraftEditOutcome =
  | { status: "saved"; view: DraftingView }
  | { status: "quarantine"; report: QuarantineReport };

export type CheckLevel = "error" | "warn" | "info";
export interface ReviewCheck {
  code: string;
  level: CheckLevel;
  args: string[];
  /** What fixes it: a section key, or `budget`, `schedule`, `call`. */
  target: string | null;
  /** The check in plain Spanish. */
  text: string;
}
export interface ReviewView {
  project: ProjectRow;
  report: { checks: ReviewCheck[]; errors: number; warnings: number };
}
export interface Exported {
  file_name: string;
  path: string;
}

// ---------------------------------------------------------------- security: PIN, backup, scan (ADR-019)

export interface BackupFile {
  file_name: string;
  path: string;
}
export interface ScanTable {
  table: string;
  texts: number;
  findings: number;
}
export interface ScanSummary {
  tables: ScanTable[];
  texts: number;
  findings: number;
}

// The runtime of the AI platform (ADR-003, IA-2): every request goes in through `runtimeSubmit`, and every outcome
// comes back with the id of its task, in the common contract of outcomes (the AI blueprint §6).
export type RuntimeChannel =
  | { kind: "chat" }
  | { kind: "screen"; place: string }
  | { kind: "event"; fact: string };
export type RuntimeContent =
  /** A screen names the capability and its input: no router, no model. */
  | { kind: "invoke"; capability: string; input: Record<string, unknown> }
  | { kind: "text"; text: string }
  /** The answer to what a paused task asked: the values join its input; to fields, under each full id. */
  | { kind: "answer"; values: Record<string, unknown> }
  /** Approve or reject the proposal with this fingerprint. */
  | { kind: "decision"; fingerprint: string; approve: boolean };
export interface RuntimeRequest {
  channel: RuntimeChannel;
  /** The task it continues; none opens a new one. */
  task?: string | null;
  content: RuntimeContent;
}
export type TaskState = "running" | "paused" | "done" | "denied" | "limit" | "error";
/** A field of the data dictionary (K3), as the kernel describes it: the question is drawn from it. */
export interface DictionaryField {
  /** Its id in its entity. */
  id: string;
  kind: "text" | "long_text" | "number" | "money" | "year" | "date" | "select" | "multi_select" | "yes_no" | "email" | "phone";
  /** The codes of a select or a multi-select. */
  options: string[];
  presence: "optional" | "expected" | "mandatory";
  applies_when: { when: "always" } | { when: "filled"; field: string } | { when: "any_of"; field: string; values: string[] };
  class: string;
  min: number | null;
  max: number | null;
  max_len: number | null;
  rule: "postal_code" | "rfc_moral" | "clabe" | "place_name" | null;
}
/** A field the person is asked for. The answer gives its value under `id`. */
export interface OutcomeAsked {
  /** Its full id: `muestra.note.phone`; its words live in `es-MX.ts` under it. */
  id: string;
  field: DictionaryField;
  /** Why the last answer did not pass, as the kernel's code; null the first time. */
  reason: string | null;
}
export type OutcomeAsk = { kind: "missing"; fields: OutcomeAsked[] } | { kind: "correct"; field: string; reason: string };
/** Why a proposal is asked for once more. */
export type OutcomeAgain = "changed" | "outdated" | "data_changed";
/** The way to go on after an error: an error never blocks. */
export type OutcomeNext = "use_screen" | "ask_administrator" | "start_again" | "check_history" | "try_later";
export interface OutcomeEvidence {
  source: "person" | "document" | "history" | "calculation";
  label: string;
  reference: string | null;
}
export type OutcomeBody =
  | { kind: "answer"; text: string; evidence: OutcomeEvidence[] }
  /** `withheld`: the paths the policy took out for where it goes (ADR-004), never their values; absent when none. */
  | { kind: "data"; capability: string; value: unknown; withheld?: string[] }
  | { kind: "clarification"; capability: string; ask: OutcomeAsk }
  | { kind: "proposal"; capability: string; proposal: unknown; fingerprint: string; again: OutcomeAgain | null }
  | { kind: "state"; reason: string }
  | { kind: "error"; code: string; next: OutcomeNext };
export interface Outcome {
  task: string;
  /** The step it comes from; 0 when no capability ran. */
  step: number;
  channel: RuntimeChannel;
  state: TaskState;
  /** Milliseconds since 1970 (UTC). */
  at_ms: number;
  body: OutcomeBody;
}
