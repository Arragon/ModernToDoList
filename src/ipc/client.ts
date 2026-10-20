import { invoke } from "@tauri-apps/api/core";
import { Commands } from "./commands";
import { safeInvoke, safeInvokeVoid } from "./safe";
import type { IpcResult } from "./safe";
import type {
  RuntimeInfo, ReadDocumentResponse, DocumentMetadataDto,
  ValidateDocumentResponse, AllocateTaskIdResponse,
  OpenSessionResponse, SaveDocumentResponse, SessionStatusResponse, UndoRedoResponse,
  WorkspaceInfo, DocumentInfo, IndexProgressInfo, DbStatus,
  TaskQueryResult, UpdateTaskFieldResponse, DeleteTaskResponse,
  AddTaskRequest, AddTaskResponse, MutationAck,
  ParticipantDto, DependencyDto, DependencyGraphDto, ProgressLinkDto,
  AttachmentDto, AttachmentImportResult,
  SearchResponseDto, SavedViewDto, QuickAddRequest,
  SmartViewKind, SmartViewResponse,
  GlobalShortcutSettings, ShortcutConflict,
  TaskCommentsResponse,
  TransferOperationKind, TransferTargetInfo, TransferPreviewDto,
  TransferOutcomeDto, TransferRecoveryReportDto,
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

// -- M6: Task Mutation (fail-soft: backends land incrementally) --
export async function updateTaskField(
  sessionId: number,
  taskKey: string,
  field: string,
  value: string,
): Promise<IpcResult<UpdateTaskFieldResponse>> {
  return safeInvoke<UpdateTaskFieldResponse>(Commands.UPDATE_TASK_FIELD, {
    sessionId, taskKey, field, value,
  });
}

export async function deleteTask(
  sessionId: number,
  taskKey: string,
): Promise<IpcResult<DeleteTaskResponse>> {
  return safeInvoke<DeleteTaskResponse>(Commands.DELETE_TASK, { sessionId, taskKey });
}

export async function addTask(
  request: AddTaskRequest,
): Promise<IpcResult<AddTaskResponse>> {
  return safeInvoke<AddTaskResponse>(Commands.ADD_TASK, { request });
}

export async function setTaskTags(
  sessionId: number,
  taskKey: string,
  documentId: string,
  tags: string[],
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.SET_TASK_TAGS, {
    sessionId, taskKey, documentId, tags,
  });
}

// -- M6: Participants --
export async function getParticipants(
  taskKey: string,
  documentId: string,
): Promise<IpcResult<ParticipantDto[]>> {
  return safeInvoke<ParticipantDto[]>(Commands.GET_PARTICIPANTS, { taskKey, documentId });
}

/** Distinct participant names across the workspace index (used for suggestions). */
export async function listParticipants(
  documentId?: string,
): Promise<IpcResult<string[]>> {
  return safeInvoke<string[]>(Commands.LIST_PARTICIPANTS, { documentId });
}

/** Bulk participant rows, used by the participant filter and grouping view. */
export async function listTaskParticipants(
  documentId?: string,
): Promise<IpcResult<ParticipantDto[]>> {
  return safeInvoke<ParticipantDto[]>(Commands.LIST_TASK_PARTICIPANTS, { documentId });
}

export async function addParticipant(
  sessionId: number,
  taskKey: string,
  documentId: string,
  displayName: string,
  role = "allocated_to",
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.ADD_PARTICIPANT, {
    sessionId, taskKey, documentId, displayName, role,
  });
}

export async function removeParticipant(
  sessionId: number,
  taskKey: string,
  documentId: string,
  displayName: string,
  role?: string,
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.REMOVE_PARTICIPANT, {
    sessionId, taskKey, documentId, displayName, role,
  });
}

// -- M6: Dependencies --
export async function getDependencies(
  taskKey: string,
  documentId: string,
): Promise<IpcResult<DependencyGraphDto>> {
  return safeInvoke<DependencyGraphDto>(Commands.GET_DEPENDENCIES, { taskKey, documentId });
}

/** Every dependency edge in the workspace; drives the TaskRow blocked indicator. */
export async function listDependencies(
  documentId?: string,
): Promise<IpcResult<DependencyDto[]>> {
  return safeInvoke<DependencyDto[]>(Commands.LIST_DEPENDENCIES, { documentId });
}

export async function addDependency(
  sessionId: number,
  taskKey: string,
  documentId: string,
  dependsOnKey: string,
  dependsOnDocumentId: string | null,
  depType = 0,
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.ADD_DEPENDENCY, {
    sessionId, taskKey, documentId, dependsOnKey, dependsOnDocumentId, depType,
  });
}

export async function removeDependency(
  sessionId: number,
  taskKey: string,
  documentId: string,
  dependsOnKey: string,
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.REMOVE_DEPENDENCY, {
    sessionId, taskKey, documentId, dependsOnKey,
  });
}

// -- M6: Progress Links --
export async function listProgressLinks(
  taskKey: string,
  documentId: string,
): Promise<IpcResult<ProgressLinkDto[]>> {
  return safeInvoke<ProgressLinkDto[]>(Commands.LIST_PROGRESS_LINKS, { taskKey, documentId });
}

export async function addProgressLink(
  sessionId: number,
  taskKey: string,
  documentId: string,
  label: string,
  url: string,
  provider?: string,
): Promise<IpcResult<ProgressLinkDto>> {
  return safeInvoke<ProgressLinkDto>(Commands.ADD_PROGRESS_LINK, {
    sessionId, taskKey, documentId, label, url, provider,
  });
}

export async function updateProgressLink(
  sessionId: number,
  linkId: string,
  taskKey: string,
  documentId: string,
  label: string,
  url: string,
  provider?: string,
): Promise<IpcResult<ProgressLinkDto>> {
  return safeInvoke<ProgressLinkDto>(Commands.UPDATE_PROGRESS_LINK, {
    sessionId, linkId, taskKey, documentId, label, url, provider,
  });
}

export async function removeProgressLink(
  sessionId: number,
  linkId: string,
  taskKey: string,
  documentId: string,
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.REMOVE_PROGRESS_LINK, {
    sessionId, linkId, taskKey, documentId,
  });
}

// -- M6: Attachments --
export async function listAttachments(
  taskKey: string,
  documentId: string,
): Promise<IpcResult<AttachmentDto[]>> {
  return safeInvoke<AttachmentDto[]>(Commands.LIST_ATTACHMENTS, { taskKey, documentId });
}

export async function addManagedAttachment(
  sessionId: number,
  taskKey: string,
  documentId: string,
  sourcePath: string,
  displayName?: string,
): Promise<IpcResult<AttachmentImportResult>> {
  return safeInvoke<AttachmentImportResult>(Commands.ADD_MANAGED_ATTACHMENT, {
    sessionId, taskKey, documentId, sourcePath, displayName,
  });
}

export async function linkLocalAttachment(
  sessionId: number,
  taskKey: string,
  documentId: string,
  sourcePath: string,
  displayName?: string,
): Promise<IpcResult<AttachmentImportResult>> {
  return safeInvoke<AttachmentImportResult>(Commands.LINK_LOCAL_ATTACHMENT, {
    sessionId, taskKey, documentId, sourcePath, displayName,
  });
}

export async function addUrlAttachment(
  sessionId: number,
  taskKey: string,
  documentId: string,
  url: string,
  displayName?: string,
): Promise<IpcResult<AttachmentImportResult>> {
  return safeInvoke<AttachmentImportResult>(Commands.ADD_URL_ATTACHMENT, {
    sessionId, taskKey, documentId, url, displayName,
  });
}

export async function updateAttachment(
  sessionId: number,
  attachmentId: string,
  taskKey: string,
  documentId: string,
  displayName: string,
): Promise<IpcResult<AttachmentDto>> {
  return safeInvoke<AttachmentDto>(Commands.UPDATE_ATTACHMENT, {
    sessionId, attachmentId, taskKey, documentId, displayName,
  });
}

export async function removeAttachment(
  sessionId: number,
  attachmentId: string,
  taskKey: string,
  documentId: string,
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.REMOVE_ATTACHMENT, {
    sessionId, attachmentId, taskKey, documentId,
  });
}

export async function openAttachment(
  attachmentId: string,
  taskKey: string,
  documentId: string,
): Promise<IpcResult<null>> {
  return safeInvokeVoid(Commands.OPEN_ATTACHMENT, { attachmentId, taskKey, documentId });
}

export async function revealAttachment(
  attachmentId: string,
  taskKey: string,
  documentId: string,
): Promise<IpcResult<null>> {
  return safeInvokeVoid(Commands.REVEAL_ATTACHMENT, { attachmentId, taskKey, documentId });
}

// -- M7: Task comments / description --
/** Reads a task's COMMENTSTYPE + content. Fail-soft: the inspector hides the section if unavailable. */
export async function getTaskComments(
  sessionId: number,
  taskKey: string,
): Promise<IpcResult<TaskCommentsResponse>> {
  return safeInvoke<TaskCommentsResponse>(Commands.GET_TASK_COMMENTS, { sessionId, taskKey });
}

// -- M9: Search, Saved Views, Quick Add --
export async function globalSearch(
  query: string,
  documentId?: string,
  limit = 50,
  offset = 0,
): Promise<IpcResult<SearchResponseDto>> {
  return safeInvoke<SearchResponseDto>(Commands.GLOBAL_SEARCH, {
    query, documentId, limit, offset,
  });
}

export async function listSavedViews(): Promise<IpcResult<SavedViewDto[]>> {
  return safeInvoke<SavedViewDto[]>(Commands.LIST_SAVED_VIEWS, {});
}

export async function createSavedView(
  name: string,
  predicates: SavedViewDto["predicates"],
): Promise<IpcResult<SavedViewDto>> {
  return safeInvoke<SavedViewDto>(Commands.CREATE_SAVED_VIEW, { name, predicates });
}

export async function renameSavedView(
  viewId: string,
  name: string,
): Promise<IpcResult<SavedViewDto>> {
  return safeInvoke<SavedViewDto>(Commands.RENAME_SAVED_VIEW, { viewId, name });
}

/** Replace a saved view's predicates (re-save the current filter state). */
export async function updateSavedView(
  viewId: string,
  predicates: SavedViewDto["predicates"],
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.UPDATE_SAVED_VIEW, { viewId, predicates });
}

/** Persist a new sidebar ordering of saved views. */
export async function reorderSavedViews(
  viewIds: string[],
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.REORDER_SAVED_VIEWS, { viewIds });
}

export async function deleteSavedView(viewId: string): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.DELETE_SAVED_VIEW, { viewId });
}

/**
 * Evaluate a built-in smart view against the workspace index.
 * `now` is an optional ISO date ("2026-10-15") injected for tests.
 */
export async function getSmartView(
  view: SmartViewKind,
  documentId?: string,
  now?: string,
): Promise<IpcResult<SmartViewResponse>> {
  return safeInvoke<SmartViewResponse>(Commands.GET_SMART_VIEW, {
    view, documentId, now,
  });
}

// -- M9: Global Quick Add shortcut (RD-M9-042~043) --

export async function getGlobalShortcutSettings(): Promise<IpcResult<GlobalShortcutSettings>> {
  return safeInvoke<GlobalShortcutSettings>(Commands.GET_GLOBAL_SHORTCUT_SETTINGS, {});
}

export async function setGlobalQuickAddShortcut(
  enabled: boolean,
  accelerator?: string,
): Promise<IpcResult<GlobalShortcutSettings>> {
  return safeInvoke<GlobalShortcutSettings>(Commands.SET_GLOBAL_QUICK_ADD_SHORTCUT, {
    enabled, accelerator,
  });
}

/** Duplicate shortcut bindings inside the command registry (QA-M9-012). */
export async function getShortcutConflicts(): Promise<IpcResult<ShortcutConflict[]>> {
  return safeInvoke<ShortcutConflict[]>(Commands.GET_SHORTCUT_CONFLICTS, {});
}

export async function quickAddTask(
  request: QuickAddRequest,
): Promise<IpcResult<AddTaskResponse>> {
  return safeInvoke<AddTaskResponse>(Commands.QUICK_ADD_TASK, { request });
}

// -- M8: Cross-document transfer (RD-M8-024~031) --

/** Workspace documents eligible as transfer targets (picker data). */
export async function listTransferTargets(
  excludeDocumentId?: string,
): Promise<IpcResult<TransferTargetInfo[]>> {
  return safeInvoke<TransferTargetInfo[]>(Commands.LIST_TRANSFER_TARGETS, {
    excludeDocumentId: excludeDocumentId ?? null,
  });
}

/** Side-effect-free plan preview for the picker confirmation step. */
export async function previewTransfer(
  sourceDocumentId: string,
  targetDocumentId: string,
  rootTaskKey: string,
  operation: TransferOperationKind,
): Promise<IpcResult<TransferPreviewDto>> {
  return safeInvoke<TransferPreviewDto>(Commands.PREVIEW_TRANSFER, {
    sourceDocumentId, targetDocumentId, rootTaskKey, operation,
  });
}

/** Copies a task subtree into another document; the source is never modified. */
export async function copyTaskBetweenDocuments(
  sourceDocumentId: string,
  targetDocumentId: string,
  rootTaskKey: string,
): Promise<IpcResult<TransferOutcomeDto>> {
  return safeInvoke<TransferOutcomeDto>(Commands.COPY_TASK_BETWEEN_DOCUMENTS, {
    sourceDocumentId, targetDocumentId, rootTaskKey,
  });
}

/**
 * Moves a task subtree into another document. The caller MUST NOT remove the
 * source row locally until this resolves ok — a move whose source commit did
 * not finish rejects with a `recovery_required:` error instead.
 */
export async function moveTaskBetweenDocuments(
  sourceDocumentId: string,
  targetDocumentId: string,
  rootTaskKey: string,
): Promise<IpcResult<TransferOutcomeDto>> {
  return safeInvoke<TransferOutcomeDto>(Commands.MOVE_TASK_BETWEEN_DOCUMENTS, {
    sourceDocumentId, targetDocumentId, rootTaskKey,
  });
}

/** Undoes a completed transfer using its durable undo artifacts. */
export async function undoTransfer(
  transactionId: string,
): Promise<IpcResult<MutationAck>> {
  return safeInvoke<MutationAck>(Commands.UNDO_TRANSFER, { transactionId });
}

/** Scans the transfer journal for interrupted transactions. */
export async function recoverTransfers(): Promise<IpcResult<TransferRecoveryReportDto[]>> {
  return safeInvoke<TransferRecoveryReportDto[]>(Commands.RECOVER_TRANSFERS, {});
}

/**
 * Completes an interrupted move whose target committed but whose source copy
 * was retained: removes the source subtree after explicit user confirmation.
 */
export async function completeInterruptedMove(
  transactionId: string,
): Promise<IpcResult<TransferRecoveryReportDto>> {
  return safeInvoke<TransferRecoveryReportDto>(Commands.COMPLETE_INTERRUPTED_MOVE, {
    transactionId,
  });
}
