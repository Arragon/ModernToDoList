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
