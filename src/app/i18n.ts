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
