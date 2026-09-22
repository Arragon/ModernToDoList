// -- M1: System --

export interface RuntimeInfo {
  version: string;
  data_dir: string;
  webview2_udf: string;
  portable_root: string;
}

// -- M2: Document DTOs --

export interface DocumentMetadataDto {
  project_name: string | null;
  filename: string | null;
  next_unique_id: number;
  file_version: string | null;
  app_ver: string | null;
  file_format: string | null;
}

export interface TaskDto {
  id: string;
  title: string;
  priority: number;
  risk: number;
  percent_done: number;
  status: string;
  children: string[];
}

export interface ReadDocumentResponse {
  xml_content: string;
  metadata: DocumentMetadataDto;
  tasks: TaskDto[];
  encoding: string;
  task_count: number;
}

export interface ValidationErrorDto {
  kind: string;
  message: string;
}

export interface ValidateDocumentResponse {
  valid: boolean;
  errors: ValidationErrorDto[];
}

export interface AllocateTaskIdResponse {
  id: string;
  next_unique_id: number;
}

// -- M3: Session DTOs --

export interface OpenSessionResponse {
  session_id: number;
  fingerprint_hash: string;
  fingerprint_size: number;
  initial_revision: number;
  task_count: number;
}

export interface SessionStatusResponse {
  session_id: number;
  current_revision: number;
  saved_revision: number;
  is_dirty: boolean;
  is_saving: boolean;
  save_generation: number;
  can_undo: boolean;
  can_redo: boolean;
  undo_count: number;
  redo_count: number;
}

export interface SaveDocumentResponse {
  success: boolean;
  fingerprint_hash: string | null;
  error: string | null;
  new_revision: number;
}

export interface UndoRedoResponse {
  success: boolean;
  description: string | null;
  message: string;
}

// -- M4: Workspace DTOs --

export interface WorkspaceInfo {
  id: string;
  name: string;
  root_path: string;
  document_count: number;
  db_available: boolean;
}

export interface DocumentInfo {
  id: string;
  file_path: string;
  doc_type: string;
  fingerprint: string | null;
  last_indexed: string | null;
}

export interface IndexProgressInfo {
  total_files: number;
  processed_files: number;
  total_tasks: number;
  current_file: string;
}

export interface DbStatus {
  available: boolean;
  mode: string;
  path: string;
  schema_version: number;
}

// -- M5: Task Query DTOs --

export interface TaskSummary {
  task_key: string;
  document_id: string;
  title: string;
  priority: number;
  status: string;
  percent_done: number;
  risk: number;
  start_date: string | null;
  due_date: string | null;
  completed_date: string | null;
  parent_key: string | null;
  position: number;
}

export interface TaskQueryResult {
  tasks: TaskSummary[];
  total: number;
}

// -- M6: Task Mutation DTOs --

export interface UpdateTaskFieldResponse {
  success: boolean;
  task_key: string;
  field: string;
  new_value: string;
  revision: number;
}

export interface DeleteTaskResponse {
  success: boolean;
  task_key: string;
  revision: number;
}

export interface AddTaskRequest {
  session_id: number;
  document_id: string;
  /** Pre-allocated TaskId from `allocate_task_id`; null lets the backend allocate. */
  task_key: string | null;
  title: string;
  parent_key: string | null;
  priority: number;
  status: string;
  due_date: string | null;
  start_date: string | null;
  tags: string[];
  participants: string[];
}

export interface AddTaskResponse {
  success: boolean;
  task_key: string;
  revision: number;
}

/** Generic acknowledgement for relation mutations that do not need a payload. */
export interface MutationAck {
  success: boolean;
  revision: number;
  message: string | null;
}

// -- M6: Participants --

export type ParticipantRole = "allocated_to" | "allocated_by" | "custom";

export interface ParticipantDto {
  task_key: string;
  document_id: string;
  display_name: string;
  role: ParticipantRole | string;
}

// -- M6: Dependencies --

export type DependencyRefKind = "local" | "external" | "unresolved";

export interface DependencyDto {
  /** Owning task (the one that declares the dependency). */
  task_key: string;
  document_id: string;
  /** Target of the dependency. Equals `raw_ref` when unresolved. */
  depends_on_key: string;
  depends_on_document_id: string | null;
  /** 0 = Finish-to-Finish, 1 = Start-to-Start, ... (raw DEPENDENCYTYPE). */
  dep_type: number;
  ref_kind: DependencyRefKind;
  /** Present when following this edge would close a cycle. */
  circular: boolean;
  raw_ref: string | null;
}

export interface DependencyGraphDto {
  /** Outgoing edges: tasks this task depends on. */
  outgoing: DependencyDto[];
  /** Incoming edges: tasks that depend on this task. */
  incoming: DependencyDto[];
  /** True when at least one unresolved incoming edge blocks this task. */
  blocked: boolean;
}

// -- M6: Progress Links --

export type ProgressLinkProvider =
  | "github"
  | "linear"
  | "jira"
  | "azure"
  | "gitlab"
  | "generic";

export interface ProgressLinkDto {
  id: string;
  task_key: string;
  document_id: string;
  label: string;
  url: string;
  provider: ProgressLinkProvider | string;
}

// -- M6: Attachments --

export type AttachmentKind = "managed" | "linked" | "url";
export type AttachmentStatus = "ok" | "missing" | "unverified" | "orphaned";

export interface AttachmentDto {
  id: string;
  task_key: string;
  document_id: string;
  kind: AttachmentKind;
  display_name: string;
  path_or_url: string;
  size: number | null;
  hash: string | null;
  /** False when a managed/linked file no longer exists on disk. */
  exists: boolean;
  status: AttachmentStatus | string;
}

export interface AttachmentImportResult {
  success: boolean;
  attachment: AttachmentDto | null;
  message: string | null;
}

// -- M7: Task comments / description --

export interface TaskCommentsResponse {
  task_key: string;
  /** Verbatim COMMENTSTYPE attribute value ("" when the task has none). */
  comments_type: string;
  /** Stored content: raw text for PLAIN_TEXT, HTML for HTML. */
  content: string;
  has_comments: boolean;
}

// -- M9: Search --

export interface SearchHitDto {
  task_key: string;
  document_id: string;
  title: string;
  document_path: string | null;
  matched_field: string;
  snippet: string;
  score: number;
}

export interface SearchResponseDto {
  hits: SearchHitDto[];
  total: number;
  truncated: boolean;
}

// -- M9: Saved Views --

export type ViewPredicateField =
  | "title"
  | "status"
  | "priority"
  | "tag"
  | "participant"
  | "due_date"
  | "start_date"
  | "group_by";

export type PredicateOperator =
  | "eq"
  | "neq"
  | "contains"
  | "in"
  | "before"
  | "after"
  | "relative"
  | "empty"
  | "not-empty";

export interface ViewPredicateDto {
  field: ViewPredicateField | string;
  operator: PredicateOperator | string;
  value: string | string[] | number | null;
}

export interface SavedViewDto {
  id: string;
  workspace_id: string | null;
  name: string;
  predicates: ViewPredicateDto[];
  sort_order: number;
  /** Number of tasks matching the predicates (backend-computed when available). */
  count: number | null;
}

// -- M9: Quick Add --

export interface QuickAddRequest {
  document_id: string | null;
  parent_key: string | null;
  title: string;
  tags: string[];
  participants: string[];
  priority: number;
  start_date: string | null;
  due_date: string | null;
}
