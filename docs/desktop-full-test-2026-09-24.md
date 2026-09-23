# ModernToDoList 桌面端全量功能测试报告

- 日期：2026-09-24
- 被测程序：`release/ModernToDoList-2.0.0-Portable-win-x64/ModernToDoList.exe`（含最新前端修复的重建版，构建于 03:24）
- 运行时：Tauri v2 + WebView2（Edg/153），Windows 10 x64
- 测试方式：通过 WebView2 远程调试端口（`--remote-debugging-port=9222`）用 CDP 驱动真实后端，逐条调用全部 IPC 命令；前端用 DOM 层校验。
- 测试脚本：`scripts/desktop-fulltest.mjs`（后端命令矩阵）、`scripts/cdp-eval.mjs`（DOM/单条调用）
- 测试工作区：`Data/ft-1790191975809/`（新建 → 写入 `ToDoList.tdl` → 建 2 个任务 → 保存）

## 结论速览

| 级别 | 数量 | 摘要 |
|---|---|---|
| 致命 (Critical) | 2 | 任务列表查询必崩；相对路径导致打开会话/读取关系全部失败 |
| 高 (High) | 1 | 保存视图重命名/删除前后端参数名不匹配，功能不可用 |
| 中 (Medium) | 1 | 命令面板命令项与部分 toast 文案未本地化 |
| 通过 | 多数 | 系统/工作区/文档/会话/任务增删改/撤销重做/依赖/进度链接/附件写入/评论/搜索 等基础命令可用 |

> 说明：本轮为**后端契约 + 前端外壳**的全量覆盖。由于 BUG-1/BUG-2 会让"打开工作区后加载任务树/进入编辑会话"直接失败，**数据已加载态的 UI（任务树渲染、检查器编辑）无法在真机上跑通**，这本身就是最严重的问题。

---

## 致命缺陷

### BUG-1（Critical）：`query_tasks` 只要索引里有任务就报 SQLite 类型错误
- 现象：新建工作区、加任务、`rebuild_index`（成功索引到 2 条）后，`query_tasks` 返回：
  `SQLite error: Invalid column type Real at index: 5, name: percent_done`
- 复现：`create_workspace → serialize_and_write_document → scan_and_index → open_document_session(绝对路径) → add_task×2 → save_document_atomic → rebuild_index → query_tasks` → 报错。
- 根因（源码已定位）：
  - 建表：`src-tauri/src/infrastructure/schema.rs:49` → `percent_done REAL NOT NULL DEFAULT 0.0`
  - 读取：`src-tauri/src/commands/task_query.rs:17` 声明 `percent_done: i32`，`:82` 用 `row.get(5)?` 直接按 `i32` 取 → rusqlite 拒绝 REAL→i32。
  - 对照：`src-tauri/src/infrastructure/saved_views.rs:699` 用 `row.get::<_, f64>(5)? as i32` 是正确的 → 两处不一致。
- 影响：任务树面板通过 `task-store.loadTasks → query_tasks` 加载，**任何含任务的工作区都打不开任务列表**。
- 建议修复：`task_query.rs` 读取改为 `f64` 再转 `i32`（与 `saved_views.rs` 一致），或把列类型改为 `INTEGER`。

### BUG-2（Critical）：`list_documents` 返回相对路径，读文件的命令按进程 CWD 解析而失败
- 现象：`list_documents` 返回 `file_path = "ToDoList.tdl"`（相对工作区根）。把这些路径直接用于读文件的命令时：
  `Failed to read file 'ToDoList.tdl': 系统找不到指定的文件。 (os error 2)`
- 受影响命令（同一根因）：
  - `open_document_session`（相对路径失败；绝对路径成功，见下）
  - `get_participants`、`get_dependencies`、`list_progress_links`、`list_attachments`（按 documentId 解析出的仍是相对路径 → 全部失败）
- 关键对照证据：
  - `open_document_session(绝对路径)` → ✅ `session_id:1`
  - `open_document_session(相对路径 "ToDoList.tdl")` → ❌ os error 2
- 根因：`src-tauri/src/commands/session.rs:126` `let file_path = PathBuf::from(&path)` 直接按当前工作目录解析；而索引存的是相对根目录的路径。前端 `session-store.ensureSessionForDocument` 传入的正是 `doc.file_path`（相对）→ 真机上打开编辑会话即失败。
- 影响：无法进入编辑会话；关系类"读取"接口全部不可用。
- 建议修复：二选一——(a) 索引里 `file_path` 存绝对路径；(b) 读文件命令把相对路径按当前工作区 `root_path` 拼接后再解析。

---

## 高优先级缺陷

### BUG-3（High）：保存视图"重命名/删除"前后端参数名不匹配
- 现象：`rename_saved_view` / `delete_saved_view` 报：
  `invalid args ... command rename_saved_view missing required key id`（delete 同理）
- 根因（契约不一致）：
  - 前端 `src/ipc/client.ts`：`renameSavedView(viewId, name)` → `invoke({ viewId, name })`；`deleteSavedView(viewId)` → `invoke({ viewId })`
  - 后端 `src-tauri/src/commands/views.rs:278/300`：`rename_saved_view(id, new_name)`、`delete_saved_view(id)`
  - camelCase 映射后前端发的是 `viewId`/`name`，后端要 `id`/`newName` → 永远缺 `id`。
- 影响：保存视图的重命名、删除功能不可用。
- 建议修复：统一命名（前端改传 `id`/`newName`，或后端参数改名）。

---

## 中优先级缺陷

### BUG-4（Medium）：命令面板与部分 toast 文案未本地化
- 现象：中文态下，命令面板命令项仍为英文（`Open Workspace…`、`New Workspace…`、`Force Tree Virtualization On` 等），占位符/页脚已本地化但命令 label/description 未走 i18n；成功类 toast 仍英文：`Workspace "…" opened`、`Indexed N tasks from M files`、`Rebuilt index: N tasks`、`Added "…"`。
- 位置：`src/app/commands.ts` 的 `registerCommand({label, description})`、各 store 的 `showToast("…")`。
- 影响：中英混排，本地化不完整。
- 说明：本轮已修复 UI 外壳（标题/空状态/工具栏/弹窗/面板）的 i18n；命令注册表与 toast 文案是**剩余未覆盖项**。

---

## 全量后端命令测试结果

图例：✅ 通过 / ❌ 应用缺陷 / ⚠️ 测试传参问题（非应用 bug，见末尾"误报排除"）

### M1 系统
| 命令 | 结果 | 说明 |
|---|---|---|
| ping | ✅ | `"pong"` |
| get_runtime_info | ✅ | version 2.0.0-dev，data_dir 正确 |

### M4 工作区
| 命令 | 结果 | 说明 |
|---|---|---|
| create_workspace | ✅ | 建库 `db_available:true`，目录已创建 |
| get_workspace_status | ✅ | 返回 id/name/root_path |
| serialize_and_write_document | ✅ | 写入 `.tdl` 成功 |
| scan_and_index | ✅ | total_files 1 |
| list_documents | ✅ | 但 `file_path` 为**相对路径**（见 BUG-2） |
| get_db_status | ✅ | WalMode，schema_version 2 |
| rebuild_index | ✅ | 索引到 2 任务（但随后 query 崩，见 BUG-1） |
| close_workspace / open_workspace | ✅ | 关闭后重开 document_count 1 |

### M2 文档
| 命令 | 结果 | 说明 |
|---|---|---|
| get_document_metadata | ✅ | |
| validate_document_cmd | ✅ | valid:true |
| read_and_parse_document | ✅ | |
| allocate_task_id | ✅ | id "1", next 2 |

### M3 会话
| 命令 | 结果 | 说明 |
|---|---|---|
| open_document_session（绝对路径） | ✅ | session_id 1 |
| open_document_session（相对路径，UI 实际传法） | ❌ | os error 2（BUG-2） |
| get_session_status | ✅ | is_dirty 等字段正常 |
| close_document_session | ✅ | |

### M6 任务增删改 / M5 查询
| 命令 | 结果 | 说明 |
|---|---|---|
| add_task（父） | ✅ | task_key 1 |
| add_task（子，parent=1） | ✅ | task_key 2 |
| update_task_field priority | ✅ | |
| update_task_field status=Done | ✅ | 映射 percent_done=100 |
| update_task_field duedate | ✅ | |
| set_task_tags | ✅ | |
| undo_last_command / redo_last_command | ✅ | "Set tags of task 1" |
| save_document_atomic | ✅ | new_revision 9 |
| query_tasks（索引非空时） | ❌ | SQLite percent_done Real（BUG-1） |
| get_task_tags | ✅ | 但返回 `["qa"]`——set_task_tags 设的是 alpha/beta，undo/redo 后可能回退，值存疑，见备注 |
| delete_task（子/父，父含子与关系） | ✅ | 删除成功 |

### M6 参与者 / 依赖 / 进度链接 / 附件
| 命令 | 结果 | 说明 |
|---|---|---|
| add_participant | ✅ | |
| get_participants | ❌ | 读文件失败（BUG-2） |
| list_participants / list_task_participants | ✅ | 走 DB，返回 [] |
| remove_participant | ✅ | |
| add_dependency | ✅ | |
| get_dependencies | ❌ | 读文件失败（BUG-2） |
| list_dependencies | ✅ | 走 DB，[] |
| remove_dependency | ✅ | |
| add_progress_link | ✅ | 返回 id |
| list_progress_links | ❌ | 读文件失败（BUG-2） |
| update_progress_link / remove_progress_link | ⚠️ | 我漏传 sessionId（误报排除） |
| add_url_attachment | ✅ | 返回 attachment.id |
| list_attachments | ❌ | 读文件失败（BUG-2） |
| add_managed_attachment（真实文件） | ✅ | kind managed |
| link_local_attachment（真实文件） | ✅ | kind linked |
| update_attachment / reveal_attachment / remove_attachment | ⚠️ | 我传的 attachmentId 为 null（误报排除） |

### M7 评论 / M9 搜索·视图·快速添加
| 命令 | 结果 | 说明 |
|---|---|---|
| get_task_comments | ✅ | PLAIN_TEXT，空 |
| global_search "父" / "子任务" | ✅(存疑) | 返回 hits:[] —— 因索引里任务未被成功查询/中文分词，搜索命中为空，需结合 BUG-1 复核 |
| create_saved_view | ⚠️ | 我 predicates 结构不对（误报排除） |
| list_saved_views | ✅ | [] |
| rename_saved_view / delete_saved_view | ❌ | 参数名不匹配（BUG-3） |
| quick_add_task | ⚠️ | 我用了 `text`，后端字段应为 `title`（误报排除） |

---

## 前端（外壳）测试结果
- i18n：中文态下主区域标题、空状态、检查器、工具栏 tooltip、弹窗按钮均已本地化（本轮修复已内嵌进桌面构建）；语言按钮显示"目标语言"。
- 主题切换：`data-theme` light/dark 正常。
- Web 降级提示：桌面端 `backendAvailable=true`，"Web 预览"横幅正确隐藏。
- 模态叠加防护：确认框/路径框打开时 Ctrl+K 不再叠加命令面板。
- 确认框 Esc 关闭、路径框输入框 id/name、favicon：已修复并随构建生效。
- 备注：本轮用**合成 KeyboardEvent** 触发 Ctrl+K 未打开面板（同代码在 Web 端经真实按键验证可用），判定为测试手法限制，非缺陷。

---

## 误报排除（这些是我测试脚本传参问题，非应用 bug）
- `update_task_field field='name'` → 后端字段名是 `title`（`task_edit.rs:289`），我传错。
- `update_progress_link` / `remove_progress_link` → 我漏传 `sessionId`。
- `update_attachment` / `reveal_attachment` / `remove_attachment` → 我从 `add_url_attachment` 的嵌套响应里取 `attachment.id` 失败，传了 null。
- `create_saved_view` → `PredicateNode` 是带单键的枚举，我给的普通对象结构不对。
- `quick_add_task` → `QuickAddRequest` 字段是 `title`，我传了 `text`。

## 待复核 / 未覆盖
- `get_task_tags` 返回 `["qa"]` 与 `set_task_tags(["alpha","beta"])` 不一致，疑似 undo/redo 或索引刷新时序问题，需单独复现。
- 数据已加载态的 UI（任务树、检查器编辑、拖拽、虚拟列表）因 BUG-1/BUG-2 无法在真机走通，待后端修复后再测。
- 原生文件夹选择框（`plugin:dialog`）为 OS 窗口，CDP 不可驱动，未纳入自动化。

## 建议修复顺序
1. BUG-1（`task_query.rs` 按 f64 读 percent_done）——否则任务树永远打不开。
2. BUG-2（统一 file_path 为绝对，或读文件命令按 root 解析）——否则无法进入编辑。
3. BUG-3（保存视图 rename/delete 参数名对齐）。
4. BUG-4（命令注册表与 toast 文案接入 i18n）。

---

# 第二部分：真实用户工作流测试（驱动 UI，非命令单测）

> 方法：用 CDP 派发**真实鼠标点击/按键**驱动桌面 App 的界面，按产品设计的主流程逐步走，关键状态用 `Page.captureScreenshot` 截图存证（`Data/shots/`）。工作区打开通过原生文件夹选择框完成。

## 场景 A：打开/新建一个空工作区 → 无法创建第一个任务（死胡同）❌
用户在空文件夹上建立工作区后：
- 工具栏"添加任务(+)""全局搜索""按参与者分组"三个按钮**全部禁用**（`:disabled="!hasDocuments"`）。
- Quick Add 组件 `v-if="hasDocuments"` → **隐藏**。
- 快捷键 `Ctrl+N` → **无任何反应**（命令 `task.addRoot` 在无文档时被 `enabled()` 判否，快捷键尊重禁用）。
- 命令面板（`Ctrl+K`）里**没有任何"新建任务"命令**（同样被禁用过滤掉）。
- 侧栏只剩"扫描并索引/重建"，对空文件夹无效。

**结论**：产品设计的"新建工作区 → 添加第一个任务"这条最基本旅程走不通——所有增任务入口都要求"已存在文档"，而第一个文档又只能由"添加任务"的 bootstrap 路径生成，形成**循环死锁**，UI 没有任何入口能打破它。
证据：`Data/shots/wf-01-empty-ws.png`。

## 场景 B：打开一个已含有效 .tdl 的工作区 → 任务树显示"No Tasks" ❌
在工作区目录放入含任务的 `.tdl`（两种都测了：① 本 App `save_document_atomic` 存出的、含 4 个任务/嵌套/依赖/PERCENTDONE 的文件；② 手写、唯一内容、2 个任务的新文件），再通过 UI：
- 点"扫描并索引"：文档能被识别（侧栏 `文档 (1)`、`ToDoList.tdl`），但状态栏 `已索引 0 个任务`，任务区显示 **"No Tasks / No tasks found in the index"**。
- 点"重建"（全量重建索引）：仍然 `已索引 0 个任务`。

**结论**：产品最核心的"打开工作区 → 查看任务列表"旅程在真机 UI 上**完全走不通**——有效任务文件被识别为 0 条。这与后端 `scan_and_index` 返回 `total_tasks:0`、以及 BUG-1/BUG-2 一致（索引/查询链路对真实工作区数据失效）。
证据：`Data/shots/wf-02-after-scan.png`、`Data/shots/wf-03-no-tasks.png`。

## 场景 C：命令面板（真实 Ctrl+K）✅（功能）/ ❌（本地化）
- `Ctrl+K` 真实按键能打开命令面板，↑↓/Enter/Esc 可用。
- 但面板内命令项 label 全英文（`Open Workspace…`、`Scan & Index`、`Rebuild Index`…）→ 印证 BUG-4。

## 因上游阻断而**无法在 UI 层验证**的旅程
由于场景 A/B 已让"任务列表加载不出来"，以下设计旅程在真机上**根本进不去**，无法走通（不是没测，是被上游卡死）：
- 选中任务 → 检查器编辑（标题/状态/优先级/日期/标签/参与者/依赖/进度链接/附件/富文本描述）。
- 任务树内联编辑、拖拽排序、虚拟列表滚动。
- 全局搜索出结果并跳转、保存视图/智能视图的创建与应用。
- 撤销/重做按钮、`Ctrl+S` 保存反馈。

## 测试卫生说明（诚实）
- 过程中我曾对**同一后端实例**并发跑过 `wf-seed.mjs`（含 `create_workspace`/`close_workspace`），把用户手动打开的工作区状态冲掉了一次，导致一度出现"UI 显示 test、后端 null"的不一致；已通过 `location.reload()` 回到干净状态后重测。教训：驱动真实 UI 时不得并发改后端状态。
- 个别后端只读诊断命令因 shell 反斜杠转义把路径弄坏（`H:\testToDoList.tdl`），该条不计入应用缺陷。

## 第二部分结论
按"真实用户应走的工作流程"衡量：**主旅程在第一步就断了**——空工作区建不了第一个任务；有任务的 `.tdl` 打开后任务树为空。当前桌面端**不具备可用的端到端用户工作流**。修复优先级仍指向 BUG-1、BUG-2（以及打破场景 A 死锁所需的"空工作区可直接新建任务/文档"入口）。

---

# 第三部分：修复与复测

## 已实施的修复
| 缺陷 | 修复 | 位置 |
|---|---|---|
| BUG-1 | `percent_done` 按 `f64` 读取再转 `i32`（列是 REAL） | `src-tauri/src/commands/task_query.rs:84` |
| BUG-2 | `resolve_document_path` JOIN `workspaces` 取 root 拼相对路径；`open_document_session` 增加 `WorkspaceState` 并对相对路径按 root 解析；`list_documents` 返回解析后的绝对路径 | `bridge.rs`、`session.rs`、`workspace.rs` |
| BUG-3 | 前端保存视图 rename/delete 改传 `{id,newName}` / `{id}` 对齐后端签名 | `src/ipc/client.ts` |
| 场景 A | `task.addRoot` 门槛 `hasDocument()`→`hasWorkspace()`；工具栏 `+`、QuickAdd 的禁用/显示条件同步放宽（`createTask` 已有 bootstrap 建首文档） | `commands.ts`、`TaskTreePanel.vue` |
| BUG-4 | 新增 toast/命令 label 的 i18n 键；app-state 成功 toast 接入 `tf()`；9 个高频命令 label 改本地化 getter | `i18n.ts`、`app-state.ts`、`commands.ts` |

## 复测结果（重建便携版后，CDP 驱动真实后端）
对含 2 个任务（`PERCENTDONE` 0 与 50）的新工作区 `Data/wf-retest-tasks`：
- `scan_and_index` → `total_tasks: 2`（修复前为 0）✅
- `query_tasks` → 返回 2 条，`percent_done` 正确为 0/50，**不再报 SQLite Real 错误**（BUG-1 修复确认）✅
- `list_documents` → 返回**绝对路径**（BUG-2 存储修复确认）✅
- `open_document_session("Retest.tdl")`（**相对路径**，即修复前 UI 的实际传法）→ 成功，`session_id:1, task_count:2`（BUG-2 解析修复确认）✅
- `vue-tsc` 与 `cargo check` 均通过；便携版重编译成功。

## 仍需人工/后续验证
- 场景 A 的**界面级**验证（空工作区点 `+`/Ctrl+N 建首任务）与"任务树在 UI 中显示"，需要 UI 真正打开一个工作区；原生文件夹选择框的自动化不稳定，此步建议人工点开工作区后由自动化接管，或后续完善选择框自动化。
- BUG-3（保存视图 rename/delete）为前端参数对齐，已通过类型检查，界面级回归待做。
