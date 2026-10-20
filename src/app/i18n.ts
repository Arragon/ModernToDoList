/**
 * Minimal i18n: a zh/en string dictionary plus a reactive `t(key)` helper.
 *
 * This is a foundation covering the main application chrome (sidebar, workspace
 * actions, toolbar, common labels and empty states). Strings not yet in the
 * dictionary fall back to the key's English value, then to the raw key, so an
 * untranslated label degrades to readable English rather than breaking.
 */
import { computed } from "vue";
import { locale, type Locale } from "../stores/ui-prefs";

type Dict = Record<string, string>;

const en: Dict = {
  "app.title": "ModernToDoList 2.0",
  "sidebar.workspace": "Workspace",
  "sidebar.openWorkspace": "Open Workspace",
  "sidebar.newWorkspace": "New Workspace",
  "sidebar.closeWorkspace": "Close Workspace",
  "sidebar.documents": "Documents",
  "sidebar.noDocuments": "No documents found — run Scan & Index.",
  "sidebar.emptyHint": "Open or create a workspace to get started",
  "sidebar.tags": "Tags",
  "sidebar.views": "Saved Views",
  "action.scanIndex": "Scan & Index",
  "action.save": "Save",
  "action.undo": "Undo",
  "action.redo": "Redo",
  "action.newTask": "New Task",
  "action.search": "Search",
  "action.commandPalette": "Command Palette",
  "prefs.language": "Language",
  "prefs.theme": "Theme",
  "prefs.theme.light": "Light",
  "prefs.theme.dark": "Dark",
  "prefs.language.zh": "中文",
  "prefs.language.en": "English",
  "empty.noSelection": "No Selection",
  "empty.selectTask": "Select a task to view and edit its properties",
  "inspector.description": "Description",
  "quickadd.placeholder": "Quick add a task… (e.g. Buy milk #shopping @home !2)",
  "action.cancel": "Cancel",
  "action.dismiss": "Dismiss",
  "transfer.copyTo": "Copy to Document…",
  "transfer.moveTo": "Move to Document…",
  "transfer.copyTitle": "Copy task to document",
  "transfer.moveTitle": "Move task to document",
  "transfer.pickTarget": "Pick a target document",
  "transfer.loadingTargets": "Loading documents…",
  "transfer.noTargets": "No other document in this workspace can receive the task.",
  "transfer.readOnly": "This document is read-only",
  "transfer.copying": "Copying",
  "transfer.moving": "Moving",
  "transfer.phase.planning": "planning…",
  "transfer.phase.staging_assets": "copying attachments",
  "transfer.phase.committing_target": "writing target document…",
  "transfer.phase.backing_up_source": "backing up source…",
  "transfer.phase.committing_source": "updating source document…",
  "transfer.phase.finishing": "finishing…",
  "transfer.completed": "Move completed",
  "transfer.copyCompleted": "Copy completed",
  "transfer.completedDetail": "{n} task(s) transferred. The transfer can be undone.",
  "transfer.moveIncomplete": "Move incomplete",
  "transfer.recoveryRequired": "Recovery required",
  "transfer.failedBeforeCommit": "Transfer failed before the target was committed",
  "transfer.sourceUnchanged": "The source document was not modified.",
  "transfer.failed": "Transfer failed",
  "transfer.recovery.title": "Interrupted transfers recovered",
  "transfer.recovery.action.rolled_back": "Rolled back",
  "transfer.recovery.action.completed": "Completed",
  "transfer.recovery.action.completed_with_duplicate": "Kept both copies",
  "transfer.recovery.action.quarantined": "Quarantined",
  "transfer.recovery.completeTitle": "Complete interrupted move?",
  "transfer.recovery.completeMessage": "The target document already holds the copied tasks. Completing the move deletes the retained source copy. This cannot be undone from here.",
  "transfer.recovery.completeAction": "Complete Move",
  "transfer.recovery.keepDuplicate": "Keep Duplicate",
  "transfer.dropConfirmTitle": "Move task to this document?",
};

const zh: Dict = {
  "app.title": "ModernToDoList 2.0",
  "sidebar.workspace": "工作区",
  "sidebar.openWorkspace": "打开工作区",
  "sidebar.newWorkspace": "新建工作区",
  "sidebar.closeWorkspace": "关闭工作区",
  "sidebar.documents": "文档",
  "sidebar.noDocuments": "未找到文档——请运行「扫描并索引」。",
  "sidebar.emptyHint": "打开或创建一个工作区以开始使用",
  "sidebar.tags": "标签",
  "sidebar.views": "保存的视图",
  "action.scanIndex": "扫描并索引",
  "action.save": "保存",
  "action.undo": "撤销",
  "action.redo": "重做",
  "action.newTask": "新建任务",
  "action.search": "搜索",
  "action.commandPalette": "命令面板",
  "prefs.language": "语言",
  "prefs.theme": "主题",
  "prefs.theme.light": "浅色",
  "prefs.theme.dark": "深色",
  "prefs.language.zh": "中文",
  "prefs.language.en": "English",
  "empty.noSelection": "未选择",
  "empty.selectTask": "选择一个任务以查看和编辑其属性",
  "inspector.description": "描述",
  "quickadd.placeholder": "快速添加任务…(例:买牛奶 #购物 @家庭 !2)",
  "action.cancel": "取消",
  "action.dismiss": "关闭",
  "transfer.copyTo": "复制到文档…",
  "transfer.moveTo": "移动到文档…",
  "transfer.copyTitle": "复制任务到文档",
  "transfer.moveTitle": "移动任务到文档",
  "transfer.pickTarget": "选择目标文档",
  "transfer.loadingTargets": "正在加载文档…",
  "transfer.noTargets": "此工作区中没有其他文档可以接收该任务。",
  "transfer.readOnly": "此文档为只读",
  "transfer.copying": "正在复制",
  "transfer.moving": "正在移动",
  "transfer.phase.planning": "规划…",
  "transfer.phase.staging_assets": "复制附件",
  "transfer.phase.committing_target": "写入目标文档…",
  "transfer.phase.backing_up_source": "备份源文档…",
  "transfer.phase.committing_source": "更新源文档…",
  "transfer.phase.finishing": "收尾…",
  "transfer.completed": "移动完成",
  "transfer.copyCompleted": "复制完成",
  "transfer.completedDetail": "已传输 {n} 个任务。此操作可撤销。",
  "transfer.moveIncomplete": "移动未完成",
  "transfer.recoveryRequired": "需要恢复",
  "transfer.failedBeforeCommit": "传输在目标提交前失败",
  "transfer.sourceUnchanged": "源文档未被修改。",
  "transfer.failed": "传输失败",
  "transfer.recovery.title": "已恢复中断的传输",
  "transfer.recovery.action.rolled_back": "已回滚",
  "transfer.recovery.action.completed": "已完成",
  "transfer.recovery.action.completed_with_duplicate": "保留了双份",
  "transfer.recovery.action.quarantined": "已隔离",
  "transfer.recovery.completeTitle": "完成中断的移动?",
  "transfer.recovery.completeMessage": "目标文档已包含复制的任务。完成移动将删除保留的源副本,此操作无法从此处撤销。",
  "transfer.recovery.completeAction": "完成移动",
  "transfer.recovery.keepDuplicate": "保留重复",
  "transfer.dropConfirmTitle": "移动任务到此文档?",
};

const dicts: Record<Locale, Dict> = { en, zh };

/** Resolves a key in the active locale, falling back to English then the key. */
export function translate(key: string, loc: Locale = locale.value): string {
  return dicts[loc][key] ?? dicts.en[key] ?? key;
}

/** Reactive translation for use in templates: `t('sidebar.openWorkspace')`. */
export function useT() {
  return computed(() => (key: string) => translate(key));
}

export function t(key: string): string {
  return translate(key);
}
