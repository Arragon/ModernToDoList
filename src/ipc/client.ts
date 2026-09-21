import { invoke } from "@tauri-apps/api/core";
import { Commands } from "./commands";
import type {
  RuntimeInfo, ReadDocumentResponse, DocumentMetadataDto,
  ValidateDocumentResponse, AllocateTaskIdResponse,
  OpenSessionResponse, SaveDocumentResponse, SessionStatusResponse, UndoRedoResponse,
  WorkspaceInfo, DocumentInfo, IndexProgressInfo, DbStatus,
  TaskQueryResult, UpdateTaskFieldResponse, DeleteTaskResponse,
} from "./types";

// -- M1: System --
export async function ping(): Promise<string> { return invoke<string>(Commands.PING); }
export async function getRuntimeInfo(): Promise<RuntimeInfo> { return invoke<RuntimeInfo>(Commands.GET_RUNTIME_INFO); }

// -- M2: Document --
export async function readAndParseDocument(path: string): Promise<ReadDocumentResponse> { return invoke<ReadDocumentResponse>(Commands.READ_AND_PARSE_DOCUMENT, { path }); }
export async function serializeAndWriteDocument(xmlContent: string, path: string, encoding?: string): Promise<void> { return invoke<void>(Commands.SERIALIZE_AND_WRITE_DOCUMENT, { xmlContent, path, encoding }); }
export async function getDocumentMetadata(path: string): Promise<DocumentMetadataDto> { return invoke<DocumentMetadataDto>(Commands.GET_DOCUMENT_METADATA, { path }); }
export async function validateDocumentCmd(path: string): Promise<ValidateDocumentResponse> { return invoke<ValidateDocumentResponse>(Commands.VALIDATE_DOCUMENT_CMD, { path }); }
export async function allocateTaskId(existingIds: string[], nextUniqueId: number): Promise<AllocateTaskIdResponse> { return invoke<AllocateTaskIdResponse>(Commands.ALLOCATE_TASK_ID, { existingIds, nextUniqueId }); }

// -- M3: Session --
export async function openDocumentSession(path: string): Promise<OpenSessionResponse> { return invoke<OpenSessionResponse>(Commands.OPEN_DOCUMENT_SESSION, { path }); }
export async function closeDocumentSession(sessionId: number): Promise<void> { return invoke<void>(Commands.CLOSE_DOCUMENT_SESSION, { sessionId }); }
export async function saveDocumentAtomic(sessionId: number): Promise<SaveDocumentResponse> { return invoke<SaveDocumentResponse>(Commands.SAVE_DOCUMENT_ATOMIC, { sessionId }); }
export async function getSessionStatus(sessionId: number): Promise<SessionStatusResponse> { return invoke<SessionStatusResponse>(Commands.GET_SESSION_STATUS, { sessionId }); }
export async function undoLastCommand(sessionId: number): Promise<UndoRedoResponse> { return invoke<UndoRedoResponse>(Commands.UNDO_LAST_COMMAND, { sessionId }); }
export async function redoLastCommand(sessionId: number): Promise<UndoRedoResponse> { return invoke<UndoRedoResponse>(Commands.REDO_LAST_COMMAND, { sessionId }); }

// -- M4: Workspace --
export async function createWorkspace(path: string, name: string): Promise<WorkspaceInfo> { return invoke<WorkspaceInfo>(Commands.CREATE_WORKSPACE, { path, name }); }
export async function openWorkspace(path: string): Promise<WorkspaceInfo> { return invoke<WorkspaceInfo>(Commands.OPEN_WORKSPACE, { path }); }
export async function closeWorkspace(): Promise<void> { return invoke<void>(Commands.CLOSE_WORKSPACE); }
export async function getWorkspaceStatus(): Promise<WorkspaceInfo | null> { return invoke<WorkspaceInfo | null>(Commands.GET_WORKSPACE_STATUS); }
export async function listDocuments(): Promise<DocumentInfo[]> { return invoke<DocumentInfo[]>(Commands.LIST_DOCUMENTS); }
export async function scanAndIndex(): Promise<IndexProgressInfo> { return invoke<IndexProgressInfo>(Commands.SCAN_AND_INDEX); }
export async function rebuildIndex(): Promise<IndexProgressInfo> { return invoke<IndexProgressInfo>(Commands.REBUILD_INDEX); }
export async function getDbStatus(): Promise<DbStatus> { return invoke<DbStatus>(Commands.GET_DB_STATUS); }

// -- M5: Task Query --
export async function queryTasks(
  documentId?: string,
  parentKey?: string,
  statusFilter?: string,
  limit?: number,
): Promise<TaskQueryResult> {
  return invoke<TaskQueryResult>(Commands.QUERY_TASKS, {
    documentId, parentKey, statusFilter, limit,
  });
}

export async function getTaskTags(taskKey: string): Promise<string[]> {
  return invoke<string[]>(Commands.GET_TASK_TAGS, { taskKey });
}

// -- M6: Task Mutation --
export async function updateTaskField(
  sessionId: number,
  taskKey: string,
  field: string,
  value: string,
): Promise<UpdateTaskFieldResponse> {
  return invoke<UpdateTaskFieldResponse>(Commands.UPDATE_TASK_FIELD, {
    sessionId, taskKey, field, value,
  });
}

export async function deleteTask(
  sessionId: number,
  taskKey: string,
): Promise<DeleteTaskResponse> {
  return invoke<DeleteTaskResponse>(Commands.DELETE_TASK, { sessionId, taskKey });
}
