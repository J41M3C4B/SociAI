import { invoke } from "@tauri-apps/api/core";
import { es } from "../i18n/es-MX";
import type {
  AddNeedOutcome,
  AiCheckView,
  AiModelsView,
  AiProvider,
  AiStatusView,
  AppError,
  AppPlace,
  AttachStart,
  PlaceAnswer,
  PlaceStart,
  Expediente,
  RecordForm,
  RecordOutcome,
  BackupFile,
  ScanSummary,
  Outcome,
  RuntimeRequest,
  BudgetItemInput,
  CreateProjectOutcome,
  Decision,
  DeleteSummary,
  ConversationView,
  DraftEditOutcome,
  DraftingView,
  Exported,
  ReviewView,
  DocumentSummary,
  Found,
  EditOutcome,
  NeedsView,
  ProfileView,
  DonorKind,
  FormValues,
  FormView,
  ManualEntry,
  ManualHit,
  InstitutionLists,
  InstitutionOverview,
  ListForm,
  ListKind,
  ListOutcome,
  ProjectColor,
  ProjectRow,
  SaveProfileOutcome,
  Scores,
  StageName,
  SummaryEdit,
  UsageReport,
} from "./types";

export interface AppInfo {
  name: string;
  version: string;
}

/** Errors from Rust arrive as `{ code, message }` with a friendly Spanish message. */
export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "message" in e && "code" in e) {
    return e as AppError;
  }
  return { code: "internal", message: es.errors.generic };
}

// Typed wrappers around invoke(). One function per Rust command.
export const appInfo = () => invoke<AppInfo>("app_info");
export const profileGet = () => invoke<ProfileView | null>("profile_get");
export const institutionOverview = () => invoke<InstitutionOverview>("institution_overview");
// Forms of «Mi institución» described as data (ADR-033)
export const formGet = (id: string) => invoke<FormView>("form_get", { id });
// `used`: the suggestions of the AI the person put in their fields (IA4)
export const formSave = (id: string, values: FormValues, decision?: Decision, used: string[] = []) =>
  invoke<SaveProfileOutcome>("form_save", { id, values, decision: decision ?? null, used });
// The record of the institution (ADR-033 §5)
export const institutionRecords = () => invoke<Expediente>("institution_records");
export const institutionRecordForm = (kind: string, id: string | null) => invoke<RecordForm>("institution_record_form", { kind, id });
export const institutionRecordSave = (kind: string, id: string | null, values: FormValues, decision?: Decision) =>
  invoke<RecordOutcome>("institution_record_save", { kind, id, values, decision: decision ?? null });
export const institutionRecordAttach = (kind: string, id: string | null, path: string) =>
  invoke<AttachStart>("institution_record_attach", { kind, id, path });
export const institutionRecordPlace = (key: string, answer: PlaceAnswer) => invoke<PlaceStart>("institution_record_place", { key, answer });
export const institutionRecordDetach = (id: string) => invoke<Expediente>("institution_record_detach", { id });
export const institutionRecordOpen = (id: string) => invoke<boolean>("institution_record_open", { id });
// The board and the bank accounts of the institution (ADR-033 §3)
export const institutionLists = () => invoke<InstitutionLists>("institution_lists");
export const institutionListForm = (kind: ListKind, id: string | null) => invoke<ListForm>("institution_list_form", { kind, id });
export const institutionListSave = (kind: ListKind, id: string | null, values: FormValues) =>
  invoke<ListOutcome>("institution_list_save", { kind, id, values });
export const institutionListSetActive = (kind: ListKind, id: string, active: boolean) =>
  invoke<InstitutionLists>("institution_list_set_active", { kind, id, active });
export const institutionListDelete = (kind: ListKind, id: string) => invoke<InstitutionLists>("institution_list_delete", { kind, id });
export const institutionClabeReveal = (id: string) => invoke<string>("institution_clabe_reveal", { id });
// The suggestions per field: discarding them
export const proposalsReject = (ids: string[]) => invoke<number>("proposals_reject", { ids });
// The manual of the program (ADR-034): the «?» of each field and the search in Ayuda, offline
export const manualEntry = (id: string) => invoke<ManualEntry | null>("manual_entry", { id });
export const appPlaces = () => invoke<AppPlace[]>("app_places");
export const manualSearch = (query: string) => invoke<ManualHit[]>("manual_search", { query });
// Documents of the institution (global): the files of a call come in with their project
export const documentsList = () => invoke<DocumentSummary[]>("documents_list");
/** Looks for a question inside the documents of the institution (IA5): by words, and by meaning when it can. */
export const documentsSearch = (query: string) => invoke<Found>("documents_search", { query });
export const documentEmergencyDelete = (id: string) =>
  invoke<DeleteSummary>("document_emergency_delete", { id });
export const devLoadFixture = (name: "asilo" | "casa-hogar" | "red-comunitaria") =>
  invoke<ProfileView>("dev_load_fixture", { name });

// AI settings (the key goes to the Windows keychain and never comes back to the screen)
export const aiStatus = () => invoke<AiStatusView>("ai_status");
export const aiSetProvider = (provider: AiProvider) => invoke<void>("ai_set_provider", { provider });
/** The models of the active service and the thinking depth: swapped here, no rebuild needed. */
export const aiModels = () => invoke<AiModelsView>("ai_models");
export const aiSetModels = (light: string, strong: string, effort: string) =>
  invoke<void>("ai_set_models", { light, strong, effort });
export const aiSetKey =(provider: AiProvider, key: string) => invoke<void>("ai_set_key", { provider, key });
export const aiClearKey = (provider: AiProvider) => invoke<void>("ai_clear_key", { provider });
export const aiSetCap = (capMxn: number) => invoke<void>("ai_set_cap", { capMxn });
/** Checks the saved key and the models without using any of the day's answers. */
export const aiCheck = () => invoke<AiCheckView>("ai_check");
export const aiUsageReport = () => invoke<UsageReport>("ai_usage_report");

// Projects (frozen: no screen offers them while it is)
export const projectList = () => invoke<ProjectRow[]>("project_list");
export const projectCreate = (initialRequest: string, decision?: Decision) =>
  invoke<CreateProjectOutcome>("project_create", { initialRequest, decision: decision ?? null });
export const projectDelete = (projectId: string) => invoke<boolean>("project_delete", { projectId });
export const projectSetDonorKind = (projectId: string, kind: DonorKind | null) => invoke<boolean>("project_set_donor_kind", { projectId, kind });
export const projectSetColor = (projectId: string, color: ProjectColor | null) => invoke<boolean>("project_set_color", { projectId, color });
export const projectAdvance = (projectId: string) => invoke<ProjectRow>("project_advance", { projectId });
export const projectGoBack = (projectId: string, target: StageName) =>
  invoke<ProjectRow>("project_go_back", { projectId, target });
// The guided conversation of the diagnosis (ADR-017)
export const conversationGet = (projectId: string) => invoke<ConversationView>("conversation_get", { projectId });
export const diagnosisSummaryEdit = (projectId: string, edit: SummaryEdit, decision?: Decision) =>
  invoke<EditOutcome>("diagnosis_summary_edit", { projectId, edit, decision: decision ?? null });
export const diagnosisSummaryConfirm = (projectId: string) =>
  invoke<ConversationView>("diagnosis_summary_confirm", { projectId });

// Needs
export const needsGet = (projectId: string) => invoke<NeedsView>("needs_get", { projectId });
export const needAdd = (projectId: string, title: string, description: string, decision?: Decision) =>
  invoke<AddNeedOutcome>("need_add", { projectId, title, description, decision: decision ?? null });
export const needRate = (projectId: string, needId: string, scores: Scores) =>
  invoke<NeedsView>("need_rate", { projectId, needId, scores });
export const needSelect = (projectId: string, needId: string) =>
  invoke<NeedsView>("need_select", { projectId, needId });

// Drafting, review and the guide in Word (ADR-018)
export const draftingGet = (projectId: string) => invoke<DraftingView>("drafting_get", { projectId });
export const draftingSetAsks = (projectId: string, asks: boolean) => invoke<DraftingView>("drafting_set_asks", { projectId, asks });
export const sectionsConfirmAll = (projectId: string) => invoke<DraftingView>("sections_confirm_all", { projectId });
export const sectionSave = (projectId: string, key: string, text: string, decision?: Decision) =>
  invoke<DraftEditOutcome>("section_save", { projectId, key, text, decision: decision ?? null });
export const sectionConfirm = (projectId: string, key: string) => invoke<DraftingView>("section_confirm", { projectId, key });
export const budgetSaveItem = (projectId: string, item: BudgetItemInput, decision?: Decision) =>
  invoke<DraftEditOutcome>("budget_save_item", { projectId, item, decision: decision ?? null });
export const budgetDeleteItem = (projectId: string, itemId: string) => invoke<DraftingView>("budget_delete_item", { projectId, itemId });
export const budgetConfirm = (projectId: string) => invoke<DraftingView>("budget_confirm", { projectId });
export const scheduleSaveActivity = (
  projectId: string,
  activityId: string | null,
  title: string,
  startMonth: number,
  endMonth: number,
  decision?: Decision,
) => invoke<DraftEditOutcome>("schedule_save_activity", { projectId, activityId, title, startMonth, endMonth, decision: decision ?? null });
export const scheduleDeleteActivity = (projectId: string, activityId: string) =>
  invoke<DraftingView>("schedule_delete_activity", { projectId, activityId });
export const scheduleConfirm = (projectId: string) => invoke<DraftingView>("schedule_confirm", { projectId });
export const reviewGet = (projectId: string) => invoke<ReviewView>("review_get", { projectId });
/** Writes the guide in Word in the person's Downloads folder. */
export const guideExport = (projectId: string) => invoke<Exported>("guide_export", { projectId });

// Security: PIN, encrypted backup and the scan of everything the app keeps (ADR-019)
export const securityScan = () => invoke<ScanSummary>("security_scan");
export const backupCreate = (password: string) => invoke<BackupFile>("backup_create", { password });
/** `data` is the backup file in base64. */
export const backupRestore = (data: string, password: string) => invoke<void>("backup_restore", { data, password });

// The runtime of the AI platform (ADR-003): the one door of every request. Each outcome is also heard by the window
// as the event "task-update".
export const runtimeSubmit = (request: RuntimeRequest) => invoke<Outcome>("runtime_submit", { request });
/** The last outcome of a task of the person, or null. */
export const runtimeTask = (task: string) => invoke<Outcome | null>("runtime_task", { task });
