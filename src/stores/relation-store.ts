/**
 * M6 relation data: participants, dependencies, progress links, attachments.
 *
 * All caches are derived from the workspace index (disposable state, always
 * rebuildable). Every mutation goes through the document session so the backend
 * can record an UndoableCommand, and every IPC call is fail-soft: when a command
 * is not registered yet the editor reports it through a toast and the bound
 * control renders disabled instead of throwing an unhandled rejection.
 */
import { computed, ref } from "vue";
import type {
  AttachmentDto, AttachmentImportResult, DependencyDto,
  ParticipantDto, ProgressLinkDto,
} from "../ipc/types";
import * as ipc from "../ipc/client";
import type { IpcResult } from "../ipc/safe";
import { ipcFailureMessage } from "../ipc/safe";
import { isCommandAvailable, markCommandUnavailable } from "./capability-store";
import { showToast } from "./app-state";
import { ensureSessionForDocument, noteMutation } from "./session-store";
import { Commands } from "../ipc/commands";

export interface TaskIdentity {
  task_key: string;
  document_id: string;
}

function reportFailure<T>(result: IpcResult<T>, context: string): void {
  if (result.ok) return;
  showToast(ipcFailureMessage(result, context), result.missing ? "warning" : "error");
}

// ── Participants ──────────────────────────────────────────────────────────────

const participantsByTask = ref<Map<string, ParticipantDto[]>>(new Map());
const participantNames = ref<string[]>([]);
const participantsLoaded = ref(false);

export const participantsAvailable = computed(() => isCommandAvailable(Commands.GET_PARTICIPANTS));
export const participantSuggestions = computed(() => participantNames.value);

export function participantsFor(taskKey: string): ParticipantDto[] {
  return participantsByTask.value.get(taskKey) ?? [];
}

export function participantNamesFor(taskKey: string): string[] {
  return participantsFor(taskKey).map((p) => p.display_name);
}

/** task_key -> display names, in the shape `applyFilters` expects. */
export const participantsByTaskMap = computed<Map<string, string[]>>(() => {
  const out = new Map<string, string[]>();
  for (const [key, list] of participantsByTask.value) {
    out.set(key, list.map((p) => p.display_name));
  }
  return out;
});

/** Loads the workspace-wide participant index (bulk; used for filter + grouping). */
export async function loadParticipants(force = false): Promise<boolean> {
  if (participantsLoaded.value && !force) return true;

  const bulk = await ipc.listTaskParticipants();
  if (bulk.ok) {
    applyParticipantRows(bulk.value);
    participantsLoaded.value = true;
    const names = await ipc.listParticipants();
    if (names.ok) mergeNames(names.value);
    return true;
  }
  if (bulk.missing) markCommandUnavailable(Commands.LIST_TASK_PARTICIPANTS);

  // Fall back to distinct names only; per-task rows then load on demand.
  const fallback = await ipc.listParticipants();
  if (fallback.ok) {
    participantNames.value = fallback.value;
    participantsLoaded.value = true;
    return true;
  }
  if (fallback.missing) markCommandUnavailable(Commands.LIST_PARTICIPANTS);
  return false;
}

/** Loads participants for a single task (inspector on-demand). */
export async function loadTaskParticipants(
  task: TaskIdentity,
  force = false,
): Promise<ParticipantDto[]> {
  if (!force) {
    const cached = participantsByTask.value.get(task.task_key);
    if (cached) return cached;
  }
  const result = await ipc.getParticipants(task.task_key, task.document_id);
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.GET_PARTICIPANTS);
    return participantsFor(task.task_key);
  }
  setParticipants(task.task_key, result.value);
  mergeNames(result.value.map((p) => p.display_name));
  return result.value;
}

function setParticipants(taskKey: string, rows: ParticipantDto[]): void {
  const next = new Map(participantsByTask.value);
  next.set(taskKey, rows);
  participantsByTask.value = next;
}

function applyParticipantRows(rows: ParticipantDto[]): void {
  const next = new Map(participantsByTask.value);
  for (const row of rows) {
    const list = next.get(row.task_key) ?? [];
    if (!list.some((p) => p.display_name === row.display_name && p.role === row.role)) {
      list.push(row);
    }
    next.set(row.task_key, list);
  }
  participantsByTask.value = next;
  mergeNames(rows.map((r) => r.display_name));
}

function mergeNames(names: string[]): void {
  const merged = new Set(participantNames.value);
  for (const name of names) {
    const trimmed = name.trim();
    if (trimmed) merged.add(trimmed);
  }
  participantNames.value = Array.from(merged).sort((a, b) => a.localeCompare(b));
}

export async function addParticipant(task: TaskIdentity, displayName: string): Promise<boolean> {
  const name = displayName.trim();
  if (!name) return false;
  if (participantsFor(task.task_key).some((p) => p.display_name === name)) {
    showToast(`"${name}" is already a participant`, "info");
    return false;
  }
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.addParticipant(sessionId, task.task_key, task.document_id, name);
  if (!result.ok) {
    reportFailure(result, "Add participant");
    return false;
  }
  const list = participantsFor(task.task_key).concat({
    task_key: task.task_key,
    document_id: task.document_id,
    display_name: name,
    role: "allocated_to",
  });
  setParticipants(task.task_key, list);
  mergeNames([name]);
  await noteMutation(sessionId);
  return true;
}

export async function removeParticipant(task: TaskIdentity, displayName: string): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const row = participantsFor(task.task_key).find((p) => p.display_name === displayName);
  const result = await ipc.removeParticipant(
    sessionId, task.task_key, task.document_id, displayName, row?.role,
  );
  if (!result.ok) {
    reportFailure(result, "Remove participant");
    return false;
  }
  setParticipants(
    task.task_key,
    participantsFor(task.task_key).filter((p) => p.display_name !== displayName),
  );
  await noteMutation(sessionId);
  return true;
}

export interface BulkAssignResult {
  tasks: number;
  added: number;
  removed: number;
  failed: number;
}

/**
 * Bulk participant assignment (RD-M6-006).
 * `replace` = the checked names become the full participant set of each task,
 * otherwise the names are unioned in.
 */
export async function bulkAssignParticipants(
  tasks: TaskIdentity[],
  names: string[],
  replace: boolean,
): Promise<BulkAssignResult> {
  const result: BulkAssignResult = { tasks: 0, added: 0, removed: 0, failed: 0 };
  const wanted = names.map((n) => n.trim()).filter(Boolean);

  for (const task of tasks) {
    result.tasks += 1;
    const current = participantNamesFor(task.task_key);
    const toAdd = wanted.filter((n) => !current.includes(n));
    const toRemove = replace ? current.filter((n) => !wanted.includes(n)) : [];

    for (const name of toAdd) {
      if (await addParticipant(task, name)) result.added += 1;
      else result.failed += 1;
    }
    for (const name of toRemove) {
      if (await removeParticipant(task, name)) result.removed += 1;
      else result.failed += 1;
    }
  }
  return result;
}

// ── Dependencies ──────────────────────────────────────────────────────────────

const dependenciesByTask = ref<Map<string, DependencyDto[]>>(new Map());
const reverseDependencies = ref<Map<string, DependencyDto[]>>(new Map());
const dependenciesLoaded = ref(false);

export const dependenciesAvailable = computed(() => isCommandAvailable(Commands.GET_DEPENDENCIES));

/** Reactive view of the forward dependency cache (task -> outgoing edges). */
export const dependencyMap = computed<ReadonlyMap<string, DependencyDto[]>>(
  () => dependenciesByTask.value,
);

/** Reactive view of the reverse dependency cache (task -> tasks it blocks). */
export const reverseDependencyMap = computed<ReadonlyMap<string, DependencyDto[]>>(
  () => reverseDependencies.value,
);

/** Tasks this task depends on ("依赖"). */
export function outgoingDependencies(taskKey: string): DependencyDto[] {
  return dependenciesByTask.value.get(taskKey) ?? [];
}

/** Tasks that depend on this task ("阻塞了"). */
export function incomingDependencies(taskKey: string): DependencyDto[] {
  return reverseDependencies.value.get(taskKey) ?? [];
}

export type DependencyState = "ok" | "external" | "unresolved" | "circular";

export function classifyDependency(
  dep: DependencyDto,
  existsInIndex: (taskKey: string) => boolean,
): DependencyState {
  if (dep.circular) return "circular";
  if (dep.ref_kind === "unresolved") return "unresolved";
  if (dep.ref_kind === "external") return "external";
  if (!existsInIndex(dep.depends_on_key)) return "unresolved";
  return "ok";
}

/** True when the task waits on at least one unfinished dependency. */
export function isBlockedBy(
  taskKey: string,
  isFinished: (taskKey: string) => boolean,
  existsInIndex: (taskKey: string) => boolean,
): boolean {
  return outgoingDependencies(taskKey).some((dep) => {
    const state = classifyDependency(dep, existsInIndex);
    if (state === "circular") return true;
    if (state === "unresolved") return true;
    return !isFinished(dep.depends_on_key);
  });
}

export async function loadDependencies(force = false): Promise<boolean> {
  if (dependenciesLoaded.value && !force) return true;
  const result = await ipc.listDependencies();
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.LIST_DEPENDENCIES);
    return false;
  }
  applyDependencyRows(result.value);
  dependenciesLoaded.value = true;
  return true;
}

export async function loadTaskDependencies(task: TaskIdentity, force = false): Promise<void> {
  if (!force && dependenciesByTask.value.has(task.task_key)) return;
  const result = await ipc.getDependencies(task.task_key, task.document_id);
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.GET_DEPENDENCIES);
    return;
  }
  applyDependencyRows([...result.value.outgoing, ...result.value.incoming]);
}

function applyDependencyRows(rows: DependencyDto[]): void {
  const forward = new Map(dependenciesByTask.value);
  const reverse = new Map(reverseDependencies.value);

  for (const row of rows) {
    const forwardList = forward.get(row.task_key) ?? [];
    if (!forwardList.some((d) => d.depends_on_key === row.depends_on_key)) {
      forwardList.push(row);
      forward.set(row.task_key, forwardList);
    }
    const reverseList = reverse.get(row.depends_on_key) ?? [];
    if (!reverseList.some((d) => d.task_key === row.task_key)) {
      reverseList.push(row);
      reverse.set(row.depends_on_key, reverseList);
    }
  }

  dependenciesByTask.value = forward;
  reverseDependencies.value = reverse;
}

export async function addDependency(
  task: TaskIdentity,
  dependsOn: TaskIdentity,
  depType = 0,
): Promise<boolean> {
  if (task.task_key === dependsOn.task_key && task.document_id === dependsOn.document_id) {
    showToast("A task cannot depend on itself", "warning");
    return false;
  }
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.addDependency(
    sessionId, task.task_key, task.document_id,
    dependsOn.task_key, dependsOn.document_id, depType,
  );
  if (!result.ok) {
    reportFailure(result, "Add dependency");
    return false;
  }
  applyDependencyRows([{
    task_key: task.task_key,
    document_id: task.document_id,
    depends_on_key: dependsOn.task_key,
    depends_on_document_id: dependsOn.document_id,
    dep_type: depType,
    ref_kind: "local",
    circular: false,
    raw_ref: null,
  }]);
  await noteMutation(sessionId);
  return true;
}

export async function removeDependency(dep: DependencyDto): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(dep.document_id);
  if (sessionId === null) return false;

  const result = await ipc.removeDependency(
    sessionId, dep.task_key, dep.document_id, dep.depends_on_key,
  );
  if (!result.ok) {
    reportFailure(result, "Remove dependency");
    return false;
  }
  const forward = new Map(dependenciesByTask.value);
  forward.set(
    dep.task_key,
    (forward.get(dep.task_key) ?? []).filter((d) => d.depends_on_key !== dep.depends_on_key),
  );
  dependenciesByTask.value = forward;

  const reverse = new Map(reverseDependencies.value);
  reverse.set(
    dep.depends_on_key,
    (reverse.get(dep.depends_on_key) ?? []).filter((d) => d.task_key !== dep.task_key),
  );
  reverseDependencies.value = reverse;

  await noteMutation(sessionId);
  return true;
}

// ── Progress links ────────────────────────────────────────────────────────────

const progressLinksByTask = ref<Map<string, ProgressLinkDto[]>>(new Map());

export const progressLinksAvailable = computed(() => isCommandAvailable(Commands.LIST_PROGRESS_LINKS));

export function progressLinksFor(taskKey: string): ProgressLinkDto[] {
  return progressLinksByTask.value.get(taskKey) ?? [];
}

function setProgressLinks(taskKey: string, links: ProgressLinkDto[]): void {
  const next = new Map(progressLinksByTask.value);
  next.set(taskKey, links);
  progressLinksByTask.value = next;
}

export async function loadProgressLinks(
  task: TaskIdentity,
  force = false,
): Promise<ProgressLinkDto[]> {
  if (!force) {
    const cached = progressLinksByTask.value.get(task.task_key);
    if (cached) return cached;
  }
  const result = await ipc.listProgressLinks(task.task_key, task.document_id);
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.LIST_PROGRESS_LINKS);
    return progressLinksFor(task.task_key);
  }
  setProgressLinks(task.task_key, result.value);
  return result.value;
}

export async function addProgressLink(
  task: TaskIdentity,
  label: string,
  url: string,
  provider?: string,
): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.addProgressLink(
    sessionId, task.task_key, task.document_id, label, url, provider,
  );
  if (!result.ok) {
    reportFailure(result, "Add progress link");
    return false;
  }
  setProgressLinks(task.task_key, [...progressLinksFor(task.task_key), result.value]);
  await noteMutation(sessionId);
  return true;
}

export async function updateProgressLink(
  task: TaskIdentity,
  link: ProgressLinkDto,
  label: string,
  url: string,
): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.updateProgressLink(
    sessionId, link.id, task.task_key, task.document_id, label, url, link.provider,
  );
  if (!result.ok) {
    reportFailure(result, "Update progress link");
    return false;
  }
  setProgressLinks(task.task_key, progressLinksFor(task.task_key).map((l) =>
    l.id === link.id ? result.value : l,
  ));
  await noteMutation(sessionId);
  return true;
}

export async function removeProgressLink(task: TaskIdentity, linkId: string): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.removeProgressLink(sessionId, linkId, task.task_key, task.document_id);
  if (!result.ok) {
    reportFailure(result, "Remove progress link");
    return false;
  }
  setProgressLinks(task.task_key, progressLinksFor(task.task_key).filter((l) => l.id !== linkId));
  await noteMutation(sessionId);
  return true;
}

// ── Attachments ───────────────────────────────────────────────────────────────

const attachmentsByTask = ref<Map<string, AttachmentDto[]>>(new Map());

export const attachmentsAvailable = computed(() => isCommandAvailable(Commands.LIST_ATTACHMENTS));

export function attachmentsFor(taskKey: string): AttachmentDto[] {
  return attachmentsByTask.value.get(taskKey) ?? [];
}

function setAttachments(taskKey: string, list: AttachmentDto[]): void {
  const next = new Map(attachmentsByTask.value);
  next.set(taskKey, list);
  attachmentsByTask.value = next;
}

export async function loadAttachments(
  task: TaskIdentity,
  force = false,
): Promise<AttachmentDto[]> {
  if (!force) {
    const cached = attachmentsByTask.value.get(task.task_key);
    if (cached) return cached;
  }
  const result = await ipc.listAttachments(task.task_key, task.document_id);
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.LIST_ATTACHMENTS);
    return attachmentsFor(task.task_key);
  }
  setAttachments(task.task_key, result.value);
  return result.value;
}

/** Applies a successful import result to the cache. */
function applyImportResult(
  result: IpcResult<AttachmentImportResult>,
  task: TaskIdentity,
  context: string,
): boolean {
  if (!result.ok) {
    reportFailure(result, context);
    return false;
  }
  const payload = result.value;
  const attachment = payload.attachment;
  if (!payload.success || !attachment) {
    showToast(payload.message ?? `${context} failed`, "error");
    return false;
  }
  const list = attachmentsFor(task.task_key).filter((a) => a.id !== attachment.id);
  list.push(attachment);
  setAttachments(task.task_key, list);
  return true;
}

/** Copies `sourcePath` into the per-document asset root (transactional import). */
export async function importManagedAttachment(
  task: TaskIdentity,
  sourcePath: string,
  displayName?: string,
): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;
  const result = await ipc.addManagedAttachment(
    sessionId, task.task_key, task.document_id, sourcePath, displayName,
  );
  const ok = applyImportResult(result, task, "Import attachment");
  if (ok) await noteMutation(sessionId);
  return ok;
}

/** Stores a validated path reference only — the source file is never copied. */
export async function linkLocalAttachment(
  task: TaskIdentity,
  sourcePath: string,
  displayName?: string,
): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;
  const result = await ipc.linkLocalAttachment(
    sessionId, task.task_key, task.document_id, sourcePath, displayName,
  );
  const ok = applyImportResult(result, task, "Link local file");
  if (ok) await noteMutation(sessionId);
  return ok;
}

export async function addUrlAttachment(
  task: TaskIdentity,
  url: string,
  displayName?: string,
): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;
  const result = await ipc.addUrlAttachment(
    sessionId, task.task_key, task.document_id, url, displayName,
  );
  const ok = applyImportResult(result, task, "Add URL attachment");
  if (ok) await noteMutation(sessionId);
  return ok;
}

export async function renameAttachment(
  task: TaskIdentity,
  attachment: AttachmentDto,
  displayName: string,
): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;
  const result = await ipc.updateAttachment(
    sessionId, attachment.id, task.task_key, task.document_id, displayName,
  );
  if (!result.ok) {
    reportFailure(result, "Rename attachment");
    return false;
  }
  setAttachments(task.task_key, attachmentsFor(task.task_key).map((a) =>
    a.id === attachment.id ? result.value : a,
  ));
  await noteMutation(sessionId);
  return true;
}

export async function removeAttachment(task: TaskIdentity, attachmentId: string): Promise<boolean> {
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;
  const result = await ipc.removeAttachment(sessionId, attachmentId, task.task_key, task.document_id);
  if (!result.ok) {
    reportFailure(result, "Remove attachment");
    return false;
  }
  setAttachments(task.task_key, attachmentsFor(task.task_key).filter((a) => a.id !== attachmentId));
  await noteMutation(sessionId);
  return true;
}

export async function openAttachment(task: TaskIdentity, attachment: AttachmentDto): Promise<boolean> {
  const result = await ipc.openAttachment(attachment.id, task.task_key, task.document_id);
  if (result.ok) return true;
  reportFailure(result, "Open attachment");
  return false;
}

export async function revealAttachment(task: TaskIdentity, attachment: AttachmentDto): Promise<boolean> {
  const result = await ipc.revealAttachment(attachment.id, task.task_key, task.document_id);
  if (result.ok) return true;
  reportFailure(result, "Reveal attachment");
  return false;
}
