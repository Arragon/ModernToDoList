# ModernToDoList 2.0 — 产品与详细技术架构设计书

> 文档定位：可直接作为后续任务拆分、Roadmap、Issue/PR 验收和技术评审的母文档  
> 目标平台：Windows 10/11 x64，Windows-first  
> 发行形态：Portable ZIP，解压即用，不以安装包为主发行形式  
> 技术基线：Tauri 2 + Vue 3 + TypeScript + Rust + SQLite + 本地 XML/TDL  
> 当前项目基线：`Arragon/ModernToDoList`，主分支当前实现为 Vue Global Build + 单页 HTML/CSS/JS + IndexedDB + XML 文件读写  
> 文档版本：2.0-Architecture-Draft-1  
> 日期：2026-09-13

---

## 0. 文档目的

本文不是产品概念稿，而是 ModernToDoList 下一代版本的开发基线。它需要同时回答四类问题：

1. **产品最终要做成什么**：包括功能边界、交互模型、核心工作流和明确不做的内容。
2. **数据如何保证安全**：包括 XML/TDL 保真、编码、未知字段、事务保存、外部修改、恢复、跨文件移动和附件处理。
3. **软件如何实现**：包括 Portable Windows 桌面架构、Rust Core、Vue UI、SQLite 索引、IPC、文件监控、富文本、搜索和性能策略。
4. **如何拆开发任务**：本文末尾给出模块、依赖、阶段、工作包和 Definition of Done，可直接据此生成正式 Roadmap 和 Issue。

本文中以下约束属于 **不可违反的架构原则**。后续功能设计、重构、性能优化和 Roadmap 均不得绕过：

- XML/TDL 文件是任务业务数据的最终真实来源，SQLite 不是。
- 用户已有 XML/TDL 中不被 ModernToDoList 理解的数据也必须保留。
- 不允许通过“重新生成一个简化 XML”实现保存。
- 不允许前端直接读写 XML、文件系统或 SQLite。
- 不允许为了方便引入必须常驻的后台服务、localhost 服务或终端进程。
- 正式版本必须能够 Portable 运行。
- 删除 SQLite 索引和缓存后，必须能够仅靠工作区文件重建全部业务数据。
- 跨文件 Move 的故障偏向必须是“可能重复，绝不丢失”。
- 新功能优先复用 AbstractSpoon ToDoList 原生字段；原生格式不能表达时，再使用经过兼容性验证的扩展机制。
- 不为未经验证的未来需求提前引入服务化、插件平台、同步服务器或复杂框架。

---

# 1. 当前版本基线与必须解决的问题

## 1.1 当前实现

现版本核心文件主要为：

```text
index.html
script.js
style.css
package.json
```

当前架构特点：

- Vue 3 通过 CDN Global Build 运行。
- Tailwind CSS 通过 CDN 运行。
- Font Awesome 通过 CDN 加载。
- UI、文件操作、XML 解析、IndexedDB、Undo、筛选、搜索、拖拽全部集中在 `script.js`。
- 使用浏览器 File System Access API 打开/保存文件。
- 使用 IndexedDB 保存工作区恢复状态。
- XML DOM 节点直接挂在任务对象上。
- 当前 README 基本为空。
- 当前运行方式本质仍是 Web App，需要 Web Server/终端服务，无法提供正常桌面应用体验。

## 1.2 当前实现中不允许带入 2.0 的风险

### R-001：筛选结果复制任务对象

当前筛选树通过对象展开生成副本。筛选状态下编辑、拖拽或重新父子化可能操作副本而不是 Canonical Task。

2.0 规则：

> 任何视图只能保存 TaskKey/TaskRef，不得复制业务实体作为可编辑对象。

### R-002：XML 子树同步可能遗留旧节点

当前重建顶层 TASK 后，对原任务节点内部旧子 TASK 的清理和重建语义不完整，删除嵌套任务存在重新出现或保留脏节点的风险。

2.0 规则：

> XML Adapter 必须拥有唯一的结构修改入口；UI 不得直接操作 XML DOM。

### R-003：Comments 类型可能被强制改成纯文本

当前逻辑会写入 `COMMENTSTYPE='PLAIN_TEXT'`。如果文件原本是 HTML、RTF 或其他内容控件，会造成隐性格式损失。

2.0 规则：

> comment content type 是受保护字段；未知类型默认只读，不允许静默转换。

### R-004：XML 编码声明与真实写出编码可能不一致

当前代码创建了带 `encoding="utf-16"` 声明的 XML 字符串，但浏览器最终文本写入不等于真正 UTF-16 编码。

2.0 规则：

> 编码是 DocumentMetadata 的一部分。读取时检测，保存时保持，除非用户明确执行编码转换。

### R-005：Undo 保存整份状态

随着任务、富文本、附件元数据增加，整份状态快照会产生明显内存、序列化和延迟成本。

2.0 使用命令式 Undo/Redo。

### R-006：Web 环境导致本地集成受限

包括：

- 文件路径受沙箱限制。
- 本地附件打开体验差。
- 文件夹长期授权不稳定。
- 无法可靠做到外部文件实时监控。
- 启动依赖 Web Server。
- 无原生单实例、文件拖放、全局快捷键等体验。

因此 2.0 正式终止“浏览器作为产品运行环境”的路线。

---

# 2. 产品定位与边界

## 2.1 产品定位

ModernToDoList 2.0 定位为：

> **Windows Portable、Local-first、XML/TDL Compatible 的现代专业任务管理器。**

产品不是：

- 云协作 SaaS。
- 浏览器网页工具。
- Todoist/TickTick 的完整功能复制。
- 团队权限平台。
- 项目管理 ERP。
- 笔记软件。
- Office 文档编辑器。

产品核心竞争力：

1. XML/TDL 开放数据格式。
2. 本地文件可见、可复制、可 Git/NAS/网盘备份。
3. Portable。
4. 数据安全和兼容优先。
5. 现代化桌面任务管理体验。
6. 多任务文件组成统一工作区。
7. 快速搜索、键盘操作和低运行开销。

## 2.2 产品设计参考

参考成熟产品，但只吸收经过验证的高价值能力：

- Things：克制、Today/Upcoming、低认知负担。
- Todoist：Quick Add、快速搜索、过滤器、键盘效率。
- OmniFocus：计划日期、截止日期、Review、任务关系。
- TickTick：多视图作为后续扩展，但不复制其功能膨胀路径。
- AbstractSpoon ToDoList：XML/TDL 原生字段、任务层级、Allocated To、Dependency、File Link、Custom Attribute 等兼容能力。

---

# 3. 不可违反的产品与技术原则

## 3.1 Source of Truth

业务数据：

```text
XML / TDL
```

索引数据：

```text
SQLite
```

恢复数据：

```text
Data/recovery/
```

缓存数据：

```text
Data/cache/
Data/webview2/
```

规则：

```text
删除 index.db
    ↓
扫描 Workspace
    ↓
解析 XML/TDL
    ↓
重建全部索引
    ↓
用户业务数据无损
```

## 3.2 Portable 真正定义

满足以下全部条件才称为 Portable：

- 通过 ZIP 分发。
- 双击 EXE 直接运行。
- 不需要安装。
- 不需要管理员权限。
- 不创建 Windows Service。
- 不依赖后台 localhost Server。
- 不需要终端窗口。
- 默认不注册文件关联。
- 默认不写注册表业务配置。
- 应用设置、数据库、Recovery、WebView2 UDF 全部默认位于程序目录 `Data/`。
- 程序目录整体移动后仍可启动。
- 工作区使用相对路径时整体移动后可继续识别。
- 关闭窗口默认真正退出。
- 所有 ModernToDoList Core 进程退出。
- 不将用户任务唯一副本存入浏览器存储。

WebView2 的 User Data Folder 必须显式设置到：

```text
.\Data\webview2\
```

不得依赖系统默认 UDF 路径。

如果程序目录不可写：

- 不静默回退到 `%APPDATA%`。
- 显示明确错误。
- 提供“选择可写 Portable 数据目录”或“退出”。
- 临时模式只能由用户明确选择。

## 3.3 轻量原则

禁止仅因为“以后可能有用”引入：

- Electron。
- Node 后台服务。
- 前端直接 SQL 插件。
- Redux/Pinia 等额外全局状态框架，除非后续真实复杂度证明 Vue 原生组合式状态不够。
- 全量 UI Framework。
- 微服务。
- 云同步后端。
- 插件系统。
- CRDT。
- 实时多人协作。
- 大型 NLP。
- 默认完整 WebView2 Fixed Runtime。

---

# 4. 功能架构总览

```text
ModernToDoList
│
├── Smart Views
│   ├── Today
│   ├── Upcoming
│   ├── Overdue
│   ├── Flagged
│   ├── Unscheduled
│   └── Completed
│
├── Workspace
│   ├── Managed Documents
│   ├── Linked Documents
│   ├── Folder Tree
│   └── Recent Documents
│
├── Task Management
│   ├── Task Tree
│   ├── Subtasks
│   ├── Status / Progress
│   ├── Priority
│   ├── Start Date
│   ├── Due Date
│   ├── Tags
│   ├── Participants
│   ├── Dependencies
│   ├── Rich Description
│   ├── Attachments
│   └── Progress Links
│
├── Productivity
│   ├── Quick Add
│   ├── Command Palette
│   ├── Global Search
│   ├── Saved Views
│   ├── Multi-select
│   └── Keyboard Workflow
│
├── Cross-document Operations
│   ├── Copy
│   ├── Move
│   ├── Dependency Remap
│   ├── Attachment Copy
│   └── Transaction Recovery
│
└── Data Safety
    ├── Atomic Save
    ├── External Change Detection
    ├── Undo / Redo
    ├── Recovery Journal
    ├── XML Validation
    └── Rebuild Index
```

---

# 5. 信息架构与 UI

## 5.1 主窗口

保持三栏主结构，不改成 Dashboard。

```text
┌─────────────────────────────────────────────────────────────────┐
│ App Bar / Quick Add / Search / Save State / Commands           │
├─────────────────┬──────────────────────────┬────────────────────┤
│ Sidebar         │ Task View                │ Inspector          │
│                 │                          │                    │
│ Smart Views     │ Tree/List                │ Core Fields        │
│ Workspace       │                          │ Participants       │
│ Tags            │                          │ Dependencies       │
│ Saved Views     │                          │ Description        │
│                 │                          │ Attachments        │
│                 │                          │ Progress Links     │
└─────────────────┴──────────────────────────┴────────────────────┘
```

## 5.2 Sidebar

固定区域：

```text
Today
Upcoming
Overdue
Flagged

Workspaces
  ▼ Work
      Project A
      Project B
  ▼ Research
      CVPR
      NodeBox

Tags
Participants
Saved Views
```

原则：

- Sidebar 中的 Smart View 是“视图”，不是新的数据容器。
- 一个 Task 可以同时出现在 Today、某标签、某参与者和搜索结果里。
- 所有视图引用同一个 `TaskKey`。

## 5.3 Inspector

默认只展开高频属性：

```text
Title
Status / Progress
Priority
Start Date
Due Date
Participants
Tags
Dependencies
Progress Links
Description
Attachments
```

低频属性放入：

```text
More Properties
```

例如：

- Risk。
- Allocated By。
- Time Estimate。
- Time Spent。
- Cost。
- Custom Attributes。
- Recurrence（后续）。
- 原 XML 扩展信息。

## 5.4 Quick Add

快捷键：

```text
Q
```

以及可选全局快捷键：

```text
Ctrl + Shift + A
```

第一版解析能力控制在可预测范围：

```text
完成架构评审 #Work @张三 p8 明天
```

支持：

- `#` 列表或标签，根据最终语法规范确定。
- `@` 参与者。
- `p0-p10` 优先级。
- 今天/明天/后天。
- 明确日期。
- 简单星期表达。

禁止第一版加入大模型或大型自然语言日期解析依赖。

---

# 6. Windows Desktop 与 Portable 架构

## 6.1 技术选型

### Desktop Runtime

**Tauri 2**

理由：

- 可以保留 Vue UI 的高开发效率。
- Rust 适合实现文件事务、XML、安全存储和系统能力。
- Windows 使用系统 WebView2，不必像 Electron 一样捆绑完整 Chromium。
- 支持原生窗口、快捷键、单实例等桌面能力。
- 可以将前端资源打入应用，不需要 Web Server。

### Frontend

```text
Vue 3
TypeScript
Vite
```

CSS：

优先使用：

```text
Tailwind 编译期输出
```

或自有 CSS。

禁止生产运行使用 Tailwind CDN。

### Core

```text
Rust
```

### Database

```text
SQLite + rusqlite
```

建议启用 bundled SQLite，使 Portable 包不依赖机器已有 SQLite DLL。

**不使用 Tauri SQL Plugin 向前端暴露数据库。**

数据库只属于 Rust Core。

### File Watcher

```text
notify
```

Windows 下使用 ReadDirectoryChangesW 后端。

### Rich Text

首选：

```text
Tiptap OSS Core
```

仅使用 MIT 开源组件，不使用需要商业授权的 Pro 扩展。

## 6.2 WebView2 策略

主发行：

```text
ModernToDoList-Portable-x64.zip
```

默认使用系统 Evergreen WebView2。

离线/特殊环境后续可单独提供：

```text
ModernToDoList-Portable-Offline-x64.zip
```

包含 Fixed WebView2 Runtime。

Fixed Runtime 不进入默认包，以避免无意义增加约百 MB 级别体积。

## 6.3 Portable Runtime Directory

推荐：

```text
ModernToDoList/
│
├── ModernToDoList.exe
├── LICENSES/
│
└── Data/
    ├── settings.json
    ├── index.db
    ├── index.db-wal
    ├── index.db-shm
    │
    ├── recovery/
    │   ├── documents/
    │   └── transactions/
    │
    ├── cache/
    │   ├── thumbnails/
    │   └── derived/
    │
    ├── logs/
    │
    └── webview2/
```

用户可删除：

```text
cache/
webview2/
index.db*
```

然后重新启动，业务任务不应丢失。

## 6.4 启动流程

```text
main()
  │
  ├─ Resolve executable directory
  ├─ Verify portable data directory writable
  ├─ Create Data directory if missing
  ├─ Set WebView2 data_directory = Data/webview2
  ├─ Acquire single instance
  ├─ Open SQLite
  ├─ Run schema migrations
  ├─ Recover incomplete transactions
  ├─ Load settings
  ├─ Restore workspace registry
  ├─ Start file watcher
  ├─ Create main window
  └─ Send bootstrap DTO to frontend
```

如果 SQLite 打不开：

```text
业务文件仍不受影响
```

程序进入：

```text
Index Recovery Mode
```

重建数据库。

---

# 7. 代码仓库目标结构

推荐将当前平面结构重构为：

```text
ModernToDoList/
│
├── package.json
├── vite.config.ts
├── tsconfig.json
│
├── src/
│   ├── app/
│   │   ├── bootstrap.ts
│   │   ├── routes.ts        # 若未来确实需要；第一版可以无 router
│   │   └── shortcuts.ts
│   │
│   ├── components/
│   │   ├── sidebar/
│   │   ├── task-tree/
│   │   ├── inspector/
│   │   ├── rich-text/
│   │   ├── command-palette/
│   │   └── common/
│   │
│   ├── features/
│   │   ├── workspace/
│   │   ├── task/
│   │   ├── search/
│   │   ├── participant/
│   │   ├── dependency/
│   │   ├── attachment/
│   │   ├── richtext/
│   │   └── saved-view/
│   │
│   ├── stores/
│   │   ├── app-state.ts
│   │   ├── workspace-state.ts
│   │   ├── selection-state.ts
│   │   └── view-state.ts
│   │
│   ├── ipc/
│   │   ├── client.ts
│   │   ├── commands.ts
│   │   ├── events.ts
│   │   └── types.ts
│   │
│   └── styles/
│
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/
│   │
│   └── src/
│       ├── main.rs
│       ├── app.rs
│       │
│       ├── commands/
│       │   ├── workspace.rs
│       │   ├── document.rs
│       │   ├── task.rs
│       │   ├── search.rs
│       │   ├── attachment.rs
│       │   └── system.rs
│       │
│       ├── domain/
│       │   ├── ids.rs
│       │   ├── task.rs
│       │   ├── document.rs
│       │   ├── workspace.rs
│       │   ├── dependency.rs
│       │   ├── attachment.rs
│       │   └── richtext.rs
│       │
│       ├── application/
│       │   ├── workspace_service.rs
│       │   ├── document_service.rs
│       │   ├── task_service.rs
│       │   ├── transfer_service.rs
│       │   ├── dependency_service.rs
│       │   ├── attachment_service.rs
│       │   ├── search_service.rs
│       │   ├── save_service.rs
│       │   └── recovery_service.rs
│       │
│       ├── infrastructure/
│       │   ├── xml/
│       │   │   ├── encoding.rs
│       │   │   ├── parser.rs
│       │   │   ├── model.rs
│       │   │   ├── mapper.rs
│       │   │   ├── writer.rs
│       │   │   └── validator.rs
│       │   │
│       │   ├── sqlite/
│       │   │   ├── connection.rs
│       │   │   ├── migrations.rs
│       │   │   ├── repository.rs
│       │   │   └── search.rs
│       │   │
│       │   ├── filesystem/
│       │   │   ├── atomic_write.rs
│       │   │   ├── watcher.rs
│       │   │   ├── fingerprint.rs
│       │   │   └── path.rs
│       │   │
│       │   └── recovery/
│       │
│       └── platform/
│           └── windows/
│               ├── explorer.rs
│               ├── shell_open.rs
│               └── portable.rs
│
└── tests/
    ├── fixtures/
    │   ├── xml/
    │   ├── workspaces/
    │   └── corruption/
    │
    └── e2e/
```

分层规则：

```text
UI
 ↓
IPC DTO
 ↓
Application Service
 ↓
Domain
 ↓
Infrastructure
```

反向依赖禁止。

---

# 8. 前端与 Rust Core 信任边界

## 8.1 前端禁止能力

Vue 前端不得直接：

```text
readFile(path)
writeFile(path)
deleteFile(path)
executeSql(sql)
openDb()
parseAndSaveXml()
```

前端只允许调用业务语义命令：

```text
create_task(...)
update_task(...)
move_task(...)
copy_task(...)
attach_file(...)
add_dependency(...)
open_workspace(...)
search_tasks(...)
```

## 8.2 为什么不直接暴露 FS/SQL

原因：

1. 所有数据安全规则必须只有一个实现。
2. UI Bug 不应获得任意文件写权限。
3. 事务、恢复和文件冲突检查不能被绕过。
4. SQLite 只是内部实现，不应成为 UI API。
5. 未来替换索引实现不影响前端。

## 8.3 IPC DTO

IPC 必须使用显式结构：

```rust
#[derive(Serialize, Deserialize)]
pub struct UpdateTaskRequest {
    pub document_id: DocumentId,
    pub task_id: TaskId,
    pub expected_revision: u64,
    pub patch: TaskPatch,
}
```

禁止使用：

```rust
serde_json::Value
HashMap<String, Value>
```

作为核心业务接口。

## 8.4 Revision

所有可编辑 Document 维护：

```text
document_revision
```

UI 发起修改：

```text
expected_revision = 105
```

Core 当前：

```text
revision = 106
```

则拒绝应用旧请求：

```text
REVISION_CONFLICT
```

用于防止异步 UI 请求覆盖新状态。

---

# 9. Domain Model

## 9.1 ID 设计

必须区分：

### WorkspaceId

ModernToDoList 自己生成：

```text
UUID
```

保存在 workspace metadata。

### DocumentId

稳定标识任务文件。

不能仅使用路径，因为文件会重命名和移动。

### TaskId

XML 文件内部 ID。

按字符串/Opaque Value 处理：

```rust
struct TaskId(String);
```

原因：

不能假设历史文件的 ID 一定符合我们自己生成规则。

### TaskKey

应用内部唯一引用：

```rust
struct TaskKey {
    document_id: DocumentId,
    task_id: TaskId,
}
```

所有：

- 搜索。
- 视图。
- 依赖。
- 选择。
- 跨文件引用。

使用 TaskKey。

禁止仅用：

```text
task_id
```

作为全局标识。

## 9.2 Task Canonical Model

示意：

```rust
struct Task {
    key: TaskKey,

    title: String,

    status: Option<String>,
    percent_done: u8,

    priority: Option<i16>,
    risk: Option<i16>,

    start_date: Option<LocalDate>,
    due_date: Option<LocalDate>,
    completed_at: Option<DateTime>,

    tags: Vec<String>,
    categories: Vec<String>,

    participants: Vec<ParticipantRef>,
    allocated_by: Option<String>,

    description: TaskDescription,

    attachments: Vec<AttachmentRef>,
    progress_links: Vec<ProgressLink>,

    dependencies: Vec<TaskRef>,

    parent: Option<TaskId>,
    children: Vec<TaskId>,

    custom_fields: Vec<CustomFieldValue>,

    source_meta: XmlSourceMeta,
}
```

### 重要规则

Task Model 不保存：

```text
Vue reactive object
DOM Element
FileHandle
```

Domain 和 UI 完全解耦。

---

# 10. XML/TDL Lossless Compatibility Layer

这是整个产品最高风险模块之一，应作为最先完成和最先测试的 Core 模块。

## 10.1 “无损”的定义

目标是：

### Semantic Lossless

保证：

- 未知元素不丢失。
- 未知属性不丢失。
- 属性值不改变语义。
- 未知 Comments 类型不丢失。
- Custom Attributes 不丢失。
- 元素顺序在要求有序的区域保持。
- CDATA/Text/Comment/PI 等必须保留其语义内容。
- 原文件编码保持。
- BOM 策略保持。
- 新增/删除/移动 Task 后，无关数据不被删除。

不承诺：

- 保存后字节级完全一致。
- 保留任意空白缩进的每个字符。
- 保留属性引号使用单引号还是双引号。

对于未修改文件，不应无意义触发重写，因此可以自然保持 byte-identical。

## 10.2 双模型

XML Layer 维护：

```text
LosslessXmlDocument
+
CanonicalDocument
```

### LosslessXmlDocument

保存完整 XML 结构。

示意：

```rust
enum XmlNode {
    Element(XmlElement),
    Text(String),
    CData(String),
    Comment(String),
    ProcessingInstruction(...),
}

struct XmlElement {
    name: QName,
    attributes: Vec<XmlAttribute>,
    children: Vec<XmlNode>,
}
```

未知内容同样进入树中，而不是丢弃。

### CanonicalDocument

只映射 ModernToDoList 理解的字段。

两个模型通过：

```text
XmlBinding
```

关联。

修改 Canonical Task 时：

```text
TaskPatch
   ↓
XmlBinding
   ↓
只修改对应 XmlElement / Attribute
```

禁止：

```text
Task Model
   ↓
从零生成整个简化 XML
```

## 10.3 XML Parser 选型

建议使用 `quick-xml` 的 streaming Reader/Writer 作为底层 tokenizer/parser。

但必须注意：

> quick-xml 当前本身不直接支持 UTF-16 输入。

因此必须在其前面增加 Encoding Layer。

## 10.4 Encoding Layer

读取：

```text
raw bytes
   ↓
detect BOM
   ↓
detect XML declaration
   ↓
decode
   ↓
UTF-8 internal string
   ↓
quick-xml
```

必须至少支持测试：

- UTF-8 无 BOM。
- UTF-8 BOM。
- UTF-16 LE BOM。
- UTF-16 BE BOM。
- XML declaration 与 BOM 一致。
- declaration 缺失。
- declaration 与真实数据冲突。

优先级：

```text
BOM > verified declaration > safe fallback
```

写出：

```text
LosslessXmlDocument
   ↓
UTF-8 logical XML
   ↓
encode using original encoding
   ↓
restore BOM policy
```

DocumentMetadata：

```rust
struct XmlEncodingMeta {
    encoding: XmlEncoding,
    bom: BomPolicy,
    declaration_encoding: Option<String>,
    line_ending: LineEnding,
}
```

禁止出现：

```xml
encoding="utf-16"
```

实际文件却写成 UTF-8 的情况。

## 10.5 Comments Compatibility

定义：

```rust
enum DescriptionKind {
    PlainText,
    Html,
    Markdown,
    Rtf,
    KnownExternal(String),
    Unknown(String),
}
```

处理规则：

### Plain Text

正常编辑。

### HTML

只有在完成 AbstractSpoon HTML Comments 格式验证后才允许直接 Rich Text round-trip。

### Markdown

若验证格式兼容则编辑。

### RTF

第一版默认：

```text
只读
```

直到存在经过验证的安全转换实现。

### Unknown

必须：

- 原始内容保留。
- 显示“不支持编辑此备注格式”。
- 不允许普通编辑器覆盖它。
- 可以提供“另存为/转换副本”，但必须明确告诉用户转换结果。

## 10.6 XML Schema Discovery Gate

正式实现前建立：

```text
TDL Compatibility Corpus
```

来源：

- 当前用户已有真实 XML/TDL 样本。
- AbstractSpoon 官方/资源仓库样本。
- ModernToDoList 当前创建文件。
- 人工构造边界样本。

产出文件：

```text
docs/compatibility/TDL_FIELD_MAPPING.md
```

明确：

```text
Feature
Native XML representation
Read
Write
Round-trip
AbstractSpoon interoperability
Modern extension needed?
```

以下功能在 Schema Audit 完成前不得假设底层字段表示：

- 多参与者。
- 多 File Link。
- Dependency 跨文件格式。
- Rich HTML Comments。
- Custom Attribute。
- Recurrence。
- Internal task link。

---

# 11. Document 与 Workspace 模型

## 11.1 Workspace

Workspace 是 ModernToDoList 的工作上下文，不是新的任务数据库。

```rust
struct Workspace {
    id: WorkspaceId,
    name: String,
    root_path: PathBuf,
    documents: Vec<DocumentId>,
}
```

## 11.2 Managed Document

位于 Workspace 管理范围内。

应用可安全执行：

- 重命名。
- 移动。
- 附件组织。
- 相对路径管理。
- Trash。

## 11.3 Linked Document

文件仍位于外部：

```text
D:\Legacy\old.tdl
\\NAS\Tasks\shared.xml
```

Workspace 只保存引用。

限制：

- 不自动迁移附件。
- 删除操作默认只能“从 Workspace 移除”。
- 真正删除外部文件必须二次明确确认。

## 11.4 Workspace Metadata

建议：

```text
Workspace/
└── .moderntodo/
    └── workspace.json
```

只保存工作区级 ModernToDoList 元数据，例如：

```json
{
  "workspaceId": "...",
  "formatVersion": 1
}
```

不要把任务核心字段放在 sidecar。

## 11.5 路径保存

设置中保存 Workspace 路径时：

优先：

```text
相对 EXE 的相对路径
```

如果无法相对：

```text
绝对路径
```

记录：

```json
{
  "pathType": "relativeToExe",
  "path": "../Workspace"
}
```

或：

```json
{
  "pathType": "absolute",
  "path": "E:\\Research"
}
```

路径丢失：

```text
Workspace unavailable
原路径：E:\Research

[重新定位]
[暂时忽略]
[从列表移除]
```

---

# 12. SQLite 索引架构

## 12.1 原则

SQLite：

- 不是 Source of Truth。
- 可以删除。
- 可以重建。
- 不存用户唯一业务内容。
- 不允许 UI 直接查询。

建议：

```text
Data/index.db
```

## 12.2 推荐 PRAGMA

本地普通磁盘：

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
```

如果 Data 位于 UNC/network share：

- 不依赖 WAL。
- 自动切换到兼容 journal mode。
- 显示性能提示。
- 数据库故障不能阻止直接打开业务 XML。

## 12.3 Schema

### schema_migrations

```sql
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
```

### workspaces

```sql
CREATE TABLE workspaces (
    workspace_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL,
    path_type TEXT NOT NULL,
    last_opened_at TEXT,
    created_at TEXT NOT NULL
);
```

### documents

```sql
CREATE TABLE documents (
    document_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    relative_path TEXT,
    absolute_path TEXT,
    document_type TEXT NOT NULL,
    is_managed INTEGER NOT NULL,

    file_size INTEGER,
    modified_at INTEGER,
    content_hash TEXT,

    encoding TEXT,
    bom_type TEXT,

    indexed_revision INTEGER NOT NULL DEFAULT 0,
    last_indexed_at TEXT,

    FOREIGN KEY(workspace_id)
        REFERENCES workspaces(workspace_id)
        ON DELETE CASCADE
);
```

### task_index

```sql
CREATE TABLE task_index (
    document_id TEXT NOT NULL,
    task_id TEXT NOT NULL,

    parent_task_id TEXT,

    title TEXT NOT NULL,
    status TEXT,
    percent_done INTEGER,

    priority INTEGER,
    risk INTEGER,

    start_date TEXT,
    due_date TEXT,
    completed_at TEXT,

    depth INTEGER NOT NULL,
    sibling_order INTEGER NOT NULL,

    search_text TEXT,

    PRIMARY KEY(document_id, task_id)
);
```

### task_tags

```sql
CREATE TABLE task_tags (
    document_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    tag TEXT NOT NULL,

    PRIMARY KEY(document_id, task_id, tag)
);
```

### task_participants

```sql
CREATE TABLE task_participants (
    document_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    participant TEXT NOT NULL,

    PRIMARY KEY(document_id, task_id, participant)
);
```

### task_dependencies

```sql
CREATE TABLE task_dependencies (
    source_document_id TEXT NOT NULL,
    source_task_id TEXT NOT NULL,

    target_document_id TEXT,
    target_task_id TEXT NOT NULL,

    dependency_kind TEXT NOT NULL,
    raw_reference TEXT,

    PRIMARY KEY(
        source_document_id,
        source_task_id,
        target_document_id,
        target_task_id
    )
);
```

### attachments_index

```sql
CREATE TABLE attachments_index (
    attachment_id TEXT PRIMARY KEY,

    document_id TEXT NOT NULL,
    task_id TEXT NOT NULL,

    attachment_kind TEXT NOT NULL,
    display_name TEXT,
    stored_path TEXT,
    target_url TEXT,

    size INTEGER,
    modified_at INTEGER
);
```

### progress_links_index

```sql
CREATE TABLE progress_links_index (
    link_id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    task_id TEXT NOT NULL,

    label TEXT,
    url TEXT NOT NULL,
    provider TEXT
);
```

### saved_views

Saved View 本身属于应用配置，可以存在 SQLite 和 settings backup 中。

```sql
CREATE TABLE saved_views (
    view_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    query_json TEXT NOT NULL,
    sort_json TEXT,
    created_at TEXT NOT NULL
);
```

### ui_state

```sql
CREATE TABLE ui_state (
    workspace_id TEXT PRIMARY KEY,
    state_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

### recovery_records

```sql
CREATE TABLE recovery_records (
    recovery_id TEXT PRIMARY KEY,
    document_id TEXT,
    transaction_id TEXT,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    state TEXT NOT NULL
);
```

## 12.4 Full Text Search

第一版建议 SQLite FTS5：

```sql
CREATE VIRTUAL TABLE task_fts USING fts5(
    document_id UNINDEXED,
    task_id UNINDEXED,
    title,
    description_text,
    tags,
    participants,
    tokenize = 'unicode61'
);
```

中文搜索需要单独验证 tokenizer 行为。

如果 `unicode61` 中文效果不足：

第一阶段可以使用：

- 简单 substring/filter for Chinese title。
- 自定义预分词字段。
- 或后续再引入轻量中文 tokenizer。

不得仅为了中文搜索第一版就引入 Elasticsearch/Lucene 类大型组件。

---

# 13. 索引生命周期

## 13.1 Workspace 打开

```text
scan files
  ↓
compare metadata
  ↓
new/changed documents
  ↓
parse
  ↓
update index
```

## 13.2 Fingerprint

每个 Document 维护：

```text
size
mtime
BLAKE3 hash
```

策略：

- 扫描阶段优先 size/mtime。
- 发生疑似改变时计算 hash。
- 保存前必须验证 fingerprint。
- 文件 watcher 事件只作为“需要检查”的提示，不作为唯一事实。

## 13.3 Index Failure

如果索引失败：

```text
Document 仍可以直接打开
```

UI 显示：

```text
⚠ 索引未完成
```

搜索可能暂时不包含该文件，但不阻止业务数据访问。

---

# 14. Task Tree 与 View Model

## 14.1 Canonical Tree

Rust Core 内部：

```text
Document
  ├ Task 1
  │  ├ Task 2
  │  └ Task 3
  └ Task 4
```

## 14.2 UI 不传整棵树副本

UI 获取：

```text
TaskSummary[]
```

并维护：

```text
visible rows
```

例如：

```ts
interface TaskRow {
  key: TaskKey
  depth: number
  hasChildren: boolean
  expanded: boolean
  title: string
  priority?: number
  dueDate?: string
  flags: number
}
```

## 14.3 筛选

筛选只返回：

```text
TaskKey set
```

或者 View Row。

禁止 clone Task 作为可编辑实体。

## 14.4 大数据性能

先实现：

```text
canonical tree
+
flatten visible rows
```

只有真实性能基准证明 DOM row 数量成为瓶颈后，再引入 Virtual List。

不要在第一阶段引入复杂 Virtual Tree。

---

# 15. 修改与保存模型

## 15.1 In-memory Session

打开 Document 后建立：

```rust
struct DocumentSession {
    document: CanonicalDocument,
    xml: LosslessXmlDocument,

    revision: u64,
    saved_revision: u64,

    original_fingerprint: FileFingerprint,

    undo_stack: CommandStack,
    dirty: bool,
}
```

## 15.2 修改流程

```text
UI action
   ↓
IPC command
   ↓
validate revision
   ↓
validate business invariant
   ↓
execute domain command
   ↓
update canonical model
   ↓
apply XML binding mutation
   ↓
update index delta
   ↓
revision += 1
   ↓
emit TaskChanged
```

## 15.3 Auto Save

推荐：

- 用户输入即时更新内存。
- 小型属性编辑 debounce。
- XML 文件写盘采用文档级 debounce。
- 切换文档、关闭、Ctrl+S 时强制 flush。

UI 状态：

```text
✓ 已保存
● 保存中
● 未保存
⚠ 保存失败
⚠ 外部冲突
```

## 15.4 Atomic Save

禁止直接 truncate 原文件。

Windows 保存流程：

```text
1. Compare current file fingerprint
2. Serialize to target encoding
3. Write .tmp in SAME directory
4. Flush temp file
5. Re-open temp file
6. Parse validation
7. Semantic validation
8. Optional recovery snapshot
9. Replace original with Windows atomic/replace semantics
10. Recalculate fingerprint
11. Mark saved_revision
12. Update index
```

为什么 temp 必须同目录：

- 尽量保证同一文件系统。
- Atomic replace 语义更可靠。

实现优先：

- Windows `ReplaceFileW` / 等价安全替换。
- 必要时保留短期 backup。

## 15.5 Save Failure

任何步骤失败：

- 原文件不应被破坏。
- session 保持 dirty。
- temp/recovery 文件保留供诊断。
- UI 明确显示错误。
- 用户可以“另存为”。

---

# 16. 外部文件修改检测

## 16.1 File Watcher

使用 `notify`。

Windows 后端：

```text
ReadDirectoryChangesW
```

监控：

- Workspace。
- 当前打开的 Linked Document。
- Managed Assets。

## 16.2 Watcher 不是唯一正确性机制

Watcher 可能：

- 合并事件。
- 丢事件。
- 产生重复事件。
- 自己保存也触发事件。

因此：

> 保存前 fingerprint check 是最后一道强制防线。

## 16.3 外部修改策略

### 当前无本地未保存修改

```text
external changed
  ↓
reload
  ↓
update index
  ↓
notify UI
```

可设置轻量提示。

### 当前存在本地修改

进入：

```text
Conflict State
```

禁止自动覆盖。

提供：

```text
查看外部版本
重新载入并放弃本地修改
另存本地版本
尝试字段级合并（后续）
```

第一版不必做复杂 XML 三方 Merge。

宁可明确冲突，不做错误自动合并。

---

# 17. Undo / Redo 架构

## 17.1 Command Pattern

```rust
trait UndoableCommand {
    fn execute(&mut self, ctx: &mut DocumentSession) -> Result<CommandResult>;
    fn undo(&mut self, ctx: &mut DocumentSession) -> Result<()>;
}
```

命令示例：

```text
UpdateTitle
SetPriority
SetDueDate
SetParticipants
AddTask
DeleteTask
MoveTaskWithinDocument
AddDependency
RemoveDependency
AttachFile
RemoveAttachment
EditDescription
```

## 17.2 Text 输入合并

用户连续输入：

```text
a
ab
abc
abcd
```

不能产生四个独立 Undo。

定义：

```text
Command Coalescing Window
```

同一 Task、同一 Field、连续短时间输入合并为一个命令。

## 17.3 跨文件命令

Cross-document Move 属于：

```text
Workspace Transaction
```

不直接塞进单文档普通 Undo stack。

提供事务级 Undo：

```text
MoveTaskTransaction
```

前提是涉及文件仍处于可验证状态。

---

# 18. 跨文件 Copy / Move

这是 2.0 数据风险最高的功能之一。

## 18.1 Copy

流程：

```text
Source subtree
   ↓
Deep clone logical task tree
   ↓
Allocate target task IDs
   ↓
Build ID map
   ↓
Rewrite internal dependency references
   ↓
Resolve attachment policy
   ↓
Insert into target session
   ↓
Validate
   ↓
Save target atomically
```

## 18.2 Move

Move 必须实现为：

```text
Transactional Copy + Verify + Delete
```

完整顺序：

```text
1. Read source subtree
2. Build transfer manifest
3. Allocate target IDs
4. Copy task subtree in memory
5. Rewrite internal references
6. Stage managed attachments
7. Stage target XML
8. Validate target XML
9. COMMIT TARGET
10. Confirm target persisted
11. Stage source deletion
12. Validate source XML
13. COMMIT SOURCE
14. Finalize attachment ownership
15. Finish transaction
```

关键安全原则：

> Target 先成功，Source 才允许删除。

因此最坏故障：

```text
出现重复任务
```

而不是：

```text
任务丢失
```

## 18.3 Transfer Journal

在：

```text
Data/recovery/transactions/
```

记录：

```json
{
  "transactionId": "...",
  "type": "moveTask",
  "source": {...},
  "target": {...},
  "phase": "targetCommitted",
  "createdAt": "...",
  "idMap": {...}
}
```

程序启动时发现未完成事务：

```text
Recovery Service
```

进行：

- 判断 target 是否已提交。
- 判断 source 是否已删除。
- 让用户/程序安全完成或回滚。

## 18.4 Task ID 分配

不得继续默认使用当前 Web 版随机 base36 字符串。

在 Schema Audit 后定义：

```text
TaskIdAllocator
```

若目标 TDL 使用标准数值 ID：

```text
max(existing numeric id) + 1
```

若文件格式明确允许其他 ID，则由 document adapter 决定。

Task ID 生成是兼容层职责，不由 UI 决定。

---

# 19. 参与者

## 19.1 语义

Participant 是任务元数据，不是账户。

```rust
struct ParticipantRef {
    display_name: String
}
```

不引入：

- 用户登录。
- User ID Server。
- 权限。
- 邮箱验证。
- 团队组织。

## 19.2 XML 映射

优先映射 AbstractSpoon 原生：

```text
Allocated To
```

具体 XML 字段结构由 Compatibility Audit 固化。

## 19.3 UI

```text
参与者
[张三 ×] [李四 ×] [+]
```

输入支持历史建议：

```text
Participant Index
```

## 19.4 Views

支持：

```text
参与者 = 张三
参与者为空
参与者包含任意...
```

---

# 20. Dependency Model

## 20.1 基本模型

只保存正向：

```text
A depends on B
```

`B blocks A` 动态计算。

禁止同时维护两份独立业务数据。

## 20.2 TaskRef

```rust
enum TaskRef {
    Local(TaskId),
    External {
        document: DocumentRef,
        task_id: TaskId,
    },
    Unresolved {
        raw: String,
    },
}
```

不能解析的原始 dependency：

```text
必须保留
```

不能为了“清理数据”而删除。

## 20.3 循环依赖

添加 Dependency 前：

```text
graph cycle check
```

默认禁止新建循环。

历史文件若已存在循环：

- 不修改原始数据。
- 显示警告。
- 不自动删除。

## 20.4 UI

任务详情：

```text
依赖
← 前置任务
   API Schema
   Database Migration

→ 阻塞任务
   Frontend Integration
```

## 20.5 Blocking Behavior

第一版只提示：

```text
等待 2 个前置任务
```

不强制禁止用户完成任务。

---

# 21. Progress Link

## 21.1 数据模型

```rust
struct ProgressLink {
    id: String,
    label: Option<String>,
    url: Url,
    provider: ProgressProvider,
}
```

Provider：

```text
GitHub
Linear
Jira
Notion
Web
Other
```

基于 URL 本地识别。

## 21.2 第一版不做远程 API

保存：

```text
label + URL
```

打开：

```text
Windows default browser
```

不需要：

- OAuth。
- API Token。
- 状态同步。
- 网络轮询。

## 21.3 存储

优先：

- AbstractSpoon Custom Attribute，如果验证可安全 round-trip。
- 或 ModernToDoList extension field。

**不能只存 SQLite。**

否则删除 index.db 后 Progress Link 会丢失，违反 Source of Truth 原则。

---

# 22. Rich Text Description

## 22.1 Editor

使用：

```text
Tiptap OSS
```

第一阶段仅包含：

- Paragraph。
- Heading。
- Bold。
- Italic。
- Underline。
- Strike。
- Text Color。
- Limited Font Size。
- Bullet List。
- Ordered List。
- Blockquote。
- Link。
- Inline Code / Code Block。
- Image。

禁止第一阶段：

- 表格。
- 协作。
- AI。
- Comments Collaboration。
- DOCX Pro extensions。
- 复杂页面布局。
- 任意字体大全。

## 22.2 HTML 安全

仅允许 whitelist：

```text
p
h1-h4
strong
em
u
s
span
ul
ol
li
blockquote
a
code
pre
img
br
```

样式白名单：

```text
color
font-size
text-align（如确有需要）
```

禁止：

```text
script
iframe
object
embed
on*
javascript:
```

所有外部粘贴 HTML 必须 sanitize。

## 22.3 存储格式

优先：

```text
HTML comments native representation
```

前提是通过 TDL Compatibility Audit。

如果现有任务是 Plain Text：

默认继续 Plain Text。

只有用户明确启用格式化或执行：

```text
转换为富文本
```

才进行类型升级。

不允许打开纯文本后因为 UI 使用 Tiptap 就自动变 HTML。

## 22.4 图片

粘贴或拖入：

```text
Clipboard/Image
   ↓
Rust Attachment Service
   ↓
copy to managed Assets
   ↓
return AssetRef
   ↓
insert relative image URL
```

默认不 Base64。

建议：

```text
Workspace/
└── .assets/
    └── <document-id>/
        └── images/
            └── <asset-id>.png
```

## 22.5 图片生命周期

Undo 删除图片引用时：

不要立即删除物理文件。

标记：

```text
orphan candidate
```

只有：

- 无任何文档引用。
- 不在 Undo/Recovery 中。
- 超过清理宽限期。

才允许 Asset GC 删除。

---

# 23. Attachment Architecture

## 23.1 AttachmentKind

```rust
enum AttachmentKind {
    ManagedFile,
    LinkedFile,
    Url,
}
```

## 23.2 Managed File

用户：

```text
添加附件
```

Core：

```text
source file
  ↓
copy
  ↓
Workspace asset directory
```

XML 保存相对链接。

## 23.3 Linked File

用户：

```text
链接本地文件
```

保存原路径。

文件不复制。

## 23.4 URL

保存 HTTP/HTTPS URL。

## 23.5 文件名冲突

物理文件名不要直接使用用户原文件名作为唯一 key。

推荐：

```text
<asset_uuid>_<safe_filename>
```

展示仍使用：

```text
display_name
```

## 23.6 Hash

Managed Attachment 可计算 BLAKE3：

用于：

- 完整性。
- 重复判断。
- 跨文件复制。

第一版不必自动 deduplicate 所有资产，避免引入复杂引用计数。

## 23.7 打开附件

通过 Rust/Windows Shell：

```text
ShellExecute / Tauri opener
```

不使用 WebView：

```text
window.open(file://...)
```

---

# 24. 文件管理与 Trash

## 24.1 从工作区移除

语义：

```text
Remove Reference
```

不删除真实文件。

## 24.2 删除 Managed Document

不直接永久删除。

移动到：

```text
Workspace/.moderntodo/trash/
```

包含：

- XML/TDL。
- Managed Assets。
- manifest。

支持恢复。

## 24.3 删除 Linked Document

默认不提供“一键删除真实外部文件”。

只提供：

```text
从工作区移除
```

真正删除需要显式高级操作和二次确认。

---

# 25. 搜索、筛选与 Saved View

## 25.1 搜索

搜索域：

- Title。
- Plain description text。
- Tag。
- Category。
- Participant。
- Attachment display name。
- Progress link label。

## 25.2 Filter Model

不要第一版复制 Todoist Query DSL。

定义结构化：

```ts
interface ViewQuery {
  all?: Condition[]
  any?: Condition[]
  none?: Condition[]
}
```

Condition：

```text
status
priority
startDate
dueDate
tag
participant
document
completed
hasAttachment
hasDependency
```

## 25.3 Saved View

保存：

```text
query + sort + display preference
```

它属于应用配置，不改变 XML Task。

---

# 26. Smart Views

## 26.1 Today

初始规则建议：

```text
未完成
AND
(
  start_date <= today
  OR due_date = today
  OR manually_flagged_for_today
)
```

“manually flagged for today” 是否落入 XML 需要 Schema/Custom Attribute 决策。

第一版也可以先不引入该字段，仅使用 Start/Due。

## 26.2 Upcoming

按日期分组：

```text
Today
Tomorrow
Monday
Tuesday
...
```

第一版优先 Agenda List。

不要一开始引入完整 Calendar library。

## 26.3 Overdue

```text
due_date < today
AND not completed
```

## 26.4 Unscheduled

没有 Start/Due。

---

# 27. Quick Add 与 Command Palette

## 27.1 Command Palette

```text
Ctrl + K
```

支持：

- 搜任务。
- 跳转任务。
- 打开工作区。
- 执行命令。
- 切换视图。
- 新建任务。

## 27.2 Commands

统一 Command Registry：

```ts
interface CommandDefinition {
  id: string
  title: string
  shortcut?: string
  when?: Predicate
  execute(): Promise<void>
}
```

避免每个组件自己散落快捷键。

---

# 28. Keyboard-first

目标快捷键：

```text
↑ ↓            选择任务
Enter          编辑标题 / 打开
Space          完成/取消完成
Tab            缩进
Shift+Tab      取消缩进
Ctrl+N         新任务
Ctrl+Shift+N   新子任务
Ctrl+D         Duplicate
Delete         删除
Ctrl+Z         Undo
Ctrl+Y         Redo
Ctrl+S         强制 Flush
Ctrl+K         Command Palette
Q              Quick Add
F2             Rename
```

所有快捷键：

- 可发现。
- Menu/Tooltip 可查看。
- 避免覆盖富文本编辑器标准键。

---

# 29. Frontend State Strategy

不立即引入 Pinia。

拆成：

```text
AppState
WorkspaceViewState
SelectionState
TaskViewState
DialogState
```

业务实体不以“整个 Workspace 深响应式对象”存在前端。

前端缓存：

```text
TaskSummary / TaskDetail DTO
```

Core 是业务修改裁决者。

## 29.1 Optimistic UI

简单字段可做 optimistic update：

```text
title
priority
status
```

如果 Core 拒绝：

```text
rollback + error
```

高风险操作：

```text
Move across documents
Delete document
Attachment import
```

不做未经确认的 optimistic commit。

---

# 30. IPC API 初版

## Workspace

```text
workspace.list
workspace.open
workspace.create
workspace.relocate
workspace.scan
workspace.rebuild_index
workspace.close
```

## Document

```text
document.list
document.open
document.create
document.import_managed
document.link_external
document.rename
document.move
document.remove_from_workspace
document.trash
document.flush
document.reload
```

## Task

```text
task.get
task.list_children
task.create
task.update
task.delete
task.complete
task.move
task.copy
task.transfer
task.set_participants
task.set_description
```

## Dependency

```text
dependency.add
dependency.remove
dependency.list_incoming
dependency.list_outgoing
```

## Attachment

```text
attachment.add_managed
attachment.add_link
attachment.add_url
attachment.remove
attachment.open
attachment.reveal
```

## Search

```text
search.tasks
search.suggestions
search.saved_views
```

## System

```text
system.open_path
system.reveal_path
system.get_runtime_info
system.get_storage_health
```

命令命名最终可在 Rust 中映射为 snake_case，但外部契约应固定并生成 TypeScript type。

---

# 31. Event Model

Core → UI：

```text
workspace.changed
document.changed
document.saved
document.save_failed
document.external_changed
document.conflict
task.changed
task.deleted
index.progress
index.completed
transaction.recovery_required
```

Event 只通知变化。

大量数据不通过 Event 重复广播，UI 根据 key 拉取 delta。

---

# 32. Recovery Architecture

## 32.1 Recovery 类型

### Unsaved Session Recovery

程序崩溃时恢复未 flush 修改。

### Save Recovery

Atomic write 中断。

### Transfer Recovery

跨文件 Move/Copy 中断。

### Attachment Recovery

文件复制完成但 XML 未提交。

## 32.2 Recovery Journal

```text
Data/recovery/
```

核心记录：

```text
operation
document
base fingerprint
expected output
phase
timestamp
```

## 32.3 启动恢复

程序启动：

```text
scan journals
  ↓
validate filesystem
  ↓
safe auto recovery?
  ├─ yes → recover
  └─ no  → show Recovery Center
```

## 32.4 Recovery Center

UI：

```text
检测到未完成操作

1. Work.tdl — 保存中断
2. Task move — target 已保存，source 未删除

[恢复]
[查看详情]
[保留副本]
```

---

# 33. Crash Safety Invariants

任何实现都必须满足：

### INV-001

保存失败不得破坏最后一次有效 XML。

### INV-002

Move 在任意崩溃点不得导致 source 和 target 同时不存在。

### INV-003

SQLite transaction 失败不得导致 XML 业务数据损坏。

### INV-004

Index 与 XML 不一致时，以 XML 为准并重建 Index。

### INV-005

外部文件已变化时，不得静默覆盖。

### INV-006

未知 XML 字段不得因编辑无关属性而消失。

### INV-007

不支持的 Comments 格式不得被静默转成 Plain Text。

### INV-008

删除 cache/index/webview2 目录不得删除用户任务。

---

# 34. Security

## 34.1 Tauri Capability

只暴露必要命令。

前端不得获得宽泛：

```text
fs:**/*
shell unrestricted
```

权限。

## 34.2 CSP

生产环境禁止：

```text
unsafe external CDN
```

所有 JS/CSS/Icon 本地打包。

## 34.3 URL

Progress Link/URL Attachment：

- 只允许安全 scheme whitelist。
- HTTP/HTTPS 正常。
- `javascript:` 禁止。
- `data:` 默认禁止。
- 本地文件使用专门 Attachment API。

## 34.4 Rich Text

任何 HTML：

```text
sanitize
```

再进入编辑器/预览。

## 34.5 Path Traversal

Managed Asset 路径：

```text
canonicalize
```

验证不得逃出 Workspace Asset Root。

禁止：

```text
..\..\Windows\...
```

作为 managed output path。

---

# 35. 性能架构

以下是 **目标预算，不是当前实测结果**。M2 性能基准完成后可调整。

## 35.1 基准 Fixture

建立：

```text
Small      1,000 tasks
Medium    10,000 tasks
Large     50,000 tasks
Stress   100,000 tasks
```

包含：

- 多层树。
- 标签。
- 参与者。
- 依赖。
- 富文本。
- 附件索引。

## 35.2 目标

优先保证：

- 单任务输入不能被整文件序列化阻塞。
- Search 使用索引。
- Task List 只渲染可见/展开节点。
- XML parsing/serialization 在 Rust 线程执行，不阻塞 UI WebView。
- Workspace scan 有进度并可取消。

建议初版性能门槛：

```text
普通交互：
UI 输入帧不因 Core I/O 长时间阻塞。

Search：
Medium fixture 搜索应呈即时感。

打开大文件：
可展示 loading/progressive state，不要求一次性冻结 UI 等待。

Memory：
不得因 Undo 对每次按键保存整文档快照而线性暴涨。
```

实际毫秒和内存上限在 Benchmark Milestone 根据真实机器数据固化，避免在无实测前制造伪精确指标。

---

# 36. Threading / Async

## 36.1 UI Thread

只做：

- WebView 渲染。
- 用户事件。

## 36.2 Rust

异步任务：

- Workspace scan。
- XML parse。
- Hash。
- Search。
- Attachment copy。
- Re-index。

Document mutation 需要确保同一 Session 串行化。

推荐：

```text
per-document mutation lock
```

而不是一个全局大锁。

## 36.3 Save Debounce

同一文档保存合并。

禁止并发两个 Save 操作覆盖同一文件。

---

# 37. File Watcher 去抖

Watcher Event：

```text
write
rename
write
write
```

合并为：

```text
DocumentPossiblyChanged
```

等待短 debounce 后：

```text
fingerprint
```

再决定是否 reload。

自己保存产生的事件：

通过：

```text
expected fingerprint + save generation
```

识别，不用脆弱的“忽略未来 500ms 所有事件”。

---

# 38. Logging

Portable Log：

```text
Data/logs/
```

记录：

- Core startup。
- DB migration。
- Workspace scan。
- Save error。
- Recovery。
- Transfer。
- XML parse error。

不记录：

- 完整任务内容。
- 富文本正文。
- 用户附件内容。

避免隐私泄露。

日志轮转：

```text
有限文件数量 + 大小
```

不能无限增长。

---

# 39. Diagnostics / Storage Health

设置中提供：

```text
Storage & Data
```

展示：

- Portable Data Path。
- Workspace Path。
- Documents。
- Task count。
- Attachments。
- Index status。
- Recovery status。
- WebView2 runtime info。

操作：

```text
重建索引
验证所有 XML
扫描丢失附件
打开 Data 目录
打开 Workspace
清理 Cache
导出诊断信息
```

“导出诊断信息”默认不含任务正文。

---

# 40. 更新策略

2.0 初始版本优先：

```text
手动 Portable 更新
```

形式：

```text
下载新版 ZIP
替换 EXE
保留 Data/
```

必须确保：

- `Data/` schema 自动 migration。
- 新版本可读旧 settings。
- Index 可重建。

后续可选：

```text
Portable Self-Updater
```

但要求：

- 无后台服务。
- 更新程序仅更新时短暂运行。
- 签名/hash 验证。
- 失败保留旧 EXE。

第一版不让 Auto Update 阻塞核心产品。

---

# 41. 不建议第一版实现的内容

明确延后：

- 云账号。
- 多人实时协作。
- CRDT。
- 在线同步服务器。
- AI 自动任务拆解。
- Habit。
- Pomodoro。
- 社交。
- 大型 Dashboard。
- 完整 Gantt。
- 插件市场。
- Electron。
- 完整 Calendar。
- 全功能 Markdown/Office 导入。
- RTF 自动无损转换。
- Git-like 三方 XML Merge。

其中 Calendar/Kanban/Review 属于 2.x，而不是永久不做。

---

# 42. Compatibility Tier

为防止功能扩展破坏 AbstractSpoon 兼容，所有字段必须分级。

## Tier A — Native

AbstractSpoon 原生、确认读写兼容。

优先使用。

示例候选：

- Title。
- Progress。
- Priority。
- Start/Due。
- Category/Tag。
- Allocated To。
- Dependency。
- File Link。

## Tier B — Compatible Extension

利用已验证会被 AbstractSpoon 保留的：

- Custom Attributes。
- Extension node。

用于：

- Progress Link。
- Modern-specific metadata。

## Tier C — Application-only

只允许存 UI 状态，不允许存核心业务字段。

例如：

- Sidebar width。
- selected task。
- expanded state。
- recently opened。
- saved panel layout。

规则：

> 用户删除 SQLite 后，Tier A/B 业务能力仍然存在。

---

# 43. 测试战略

## 43.1 XML Golden Tests

Fixture：

```text
utf8.xml
utf8-bom.xml
utf16le.xml
utf16be.xml
unknown-attributes.xml
unknown-elements.xml
custom-attributes.xml
plain-comments.xml
html-comments.xml
rtf-comments.xml
deep-tree.xml
dependencies.xml
attachments.xml
malformed.xml
```

## 43.2 Round-trip Tests

每个 Fixture：

### No-op

```text
load
no edit
```

不触发文件重写。

### Single Field

```text
modify TITLE
save
```

验证：

- TITLE 变化。
- 所有其他语义数据保持。

### Nested Delete

删除 child。

验证：

- child 真正消失。
- sibling 不改变。

### Move

改变父子关系。

### Unknown Field

修改 Due Date 后 unknown data 仍存在。

## 43.3 Encoding Tests

验证：

```text
input encoding == output encoding
```

以及 BOM。

## 43.4 Fault Injection

Atomic Save 每个阶段模拟：

```text
I/O error
disk full
permission denied
process crash
```

检查原文件仍有效。

## 43.5 Transfer Fault Matrix

Move：

```text
before target temp
after target temp
after target commit
before source commit
after source commit
```

每个崩溃点都必须可恢复。

## 43.6 Database Rebuild Test

```text
1. Create Workspace
2. Add tasks
3. Exit
4. Delete Data/index.db
5. Relaunch
6. Rebuild
7. Verify all business features
```

## 43.7 UI E2E

重点：

- Create task。
- Edit。
- Filter then edit。
- Drag while filtered。
- Multi select。
- Rich text。
- Attachment。
- Cross-file move。
- Conflict dialog。
- Crash recovery。

---

# 44. CI 质量门

每个 PR：

```text
frontend lint
typescript check
rust fmt
clippy
rust unit tests
xml golden tests
database migration tests
```

Core 修改必须：

```text
cargo test
```

Data Safety 模块必须额外：

```text
fault injection tests
round-trip fixtures
```

Release Candidate：

- E2E。
- Large fixture benchmark。
- Portable clean machine smoke test。
- Windows 10/11 smoke test。
- No-network smoke test。
- Read-only folder behavior test。
- UNC workspace test。

---

# 45. Portable Clean Machine 验收

准备未安装 Node/Rust 的 Windows VM。

解压：

```text
ModernToDoList-Portable-x64.zip
```

验证：

- EXE 可启动。
- 无终端。
- 无 localhost。
- 无 Node。
- 无管理员权限。
- 无安装步骤。
- 创建 Data。
- WebView2 data 位于 Data/webview2。
- 打开 Workspace。
- 编辑/保存。
- 退出无应用后台进程。
- 删除目录即完成卸载。

如果 WebView2 不存在：

显示：

```text
WebView2 Runtime required
```

并提供明确离线包说明，不静默安装系统组件。

---

# 46. Repository Migration Strategy

不建议在当前 `script.js` 内继续逐步堆 2.0。

建议：

```text
main
  = 稳定版本

branch:
  next-desktop
```

或建立明确目录后逐步合并。

## 阶段 1

创建 Tauri/Vite/Vue/TS Skeleton。

当前 Web UI 暂时作为视觉参考。

## 阶段 2

建立 Rust XML Compatibility Core。

在此阶段不急于做完整 UI。

## 阶段 3

Workspace + SQLite。

## 阶段 4

Task Tree/Inspector 移植。

## 阶段 5

新功能。

原则：

> 数据层先成熟，UI 后接入。

否则会重演当前“UI 逻辑先行、数据正确性补丁跟进”的问题。

---

# 47. Architecture Decision Records

以下决策应正式建立 ADR 文件。

## ADR-001

**Desktop Runtime：Tauri 2**

拒绝：

- Browser-only。
- Electron。
- 全量 WinUI 重写。

## ADR-002

**XML/TDL 是业务 Source of Truth**

SQLite 为索引。

## ADR-003

**Portable Data 位于程序目录 Data/**

不默认 AppData。

## ADR-004

**WebView2 UDF 指向 Data/webview2**

保证 Portable 行为。

## ADR-005

**Frontend 无直接 FS/DB 权限**

所有业务写操作通过 Rust Application Service。

## ADR-006

**Semantic Lossless XML**

未知数据必须保留。

## ADR-007

**Atomic Save**

禁止直接覆盖。

## ADR-008

**Move = Copy + Verify + Delete**

故障偏向重复而非丢失。

## ADR-009

**Rich Text：Tiptap OSS**

避免 Pro dependency。

## ADR-010

**Managed Attachments 外置文件**

不默认 Base64。

## ADR-011

**不把 SQLite 作为唯一业务存储**

数据库可重建。

## ADR-012

**不提前实现 Cloud/Sync**

---

# 48. 模块责任矩阵

| 模块 | 负责 | 不负责 |
|---|---|---|
| Vue UI | 展示、输入、交互 | 文件、SQL、XML |
| IPC | DTO/命令边界 | 业务规则 |
| Task Service | Task mutation | 物理 XML 格式细节 |
| Transfer Service | 跨文件 Copy/Move | UI |
| XML Adapter | TDL/XML 映射与保真 | Workspace UI |
| Save Service | Atomic persistence | View filter |
| SQLite Repository | Index/cache | Source of Truth |
| Search Service | Query/index | XML mutation |
| Attachment Service | 文件复制、路径、打开 | Rich Text rendering |
| Recovery Service | Crash/transaction recovery | 普通 UI state |
| File Watcher | 变化通知 | 最终冲突裁决 |
| Rich Text UI | 编辑 HTML | 任意本地文件访问 |

---

# 49. Error Model

Core 错误不要直接把 Rust debug 字符串发给 UI。

定义：

```rust
enum AppErrorCode {
    FileNotFound,
    PermissionDenied,
    XmlMalformed,
    UnsupportedEncoding,
    UnsupportedCommentType,
    ExternalModificationConflict,
    RevisionConflict,
    DependencyCycle,
    InvalidTransfer,
    AttachmentMissing,
    DatabaseUnavailable,
    RecoveryRequired,
}
```

IPC：

```json
{
  "code": "EXTERNAL_MODIFICATION_CONFLICT",
  "message": "文件已被外部修改",
  "context": {
    "documentId": "..."
  }
}
```

日志可记录底层 cause。

UI 显示人类可理解的信息。

---

# 50. Task Decomposition Ready：Epic 划分

以下 Epic 可以直接用于后续 Roadmap。

## E00 — Baseline Freeze & Compatibility Corpus

目标：

建立 2.0 开发的真实性基线。

任务：

- E00-T01 固定当前 repo baseline commit。
- E00-T02 收集真实 TDL/XML 样本。
- E00-T03 收集不同 encoding 样本。
- E00-T04 调研 AbstractSpoon 字段映射。
- E00-T05 建立 `TDL_FIELD_MAPPING.md`。
- E00-T06 建立 round-trip golden fixtures。
- E00-T07 明确 Task ID 分配规则。
- E00-T08 明确 HTML/RTF comment 表示。
- E00-T09 明确多 file link 表示。
- E00-T10 明确跨文件 dependency 表示。

Definition of Done：

- 所有 P0 格式未知项都有实证答案。
- Fixture 进入 repo。
- 任何 XML Core 开发均有测试输入。

依赖：

无。

---

## E01 — Windows Portable Foundation

任务：

- E01-T01 建立 Vue3 + TS + Vite。
- E01-T02 建立 Tauri 2。
- E01-T03 Rust Core skeleton。
- E01-T04 Portable root resolver。
- E01-T05 Data writable check。
- E01-T06 WebView2 Data directory 指向 `Data/webview2`。
- E01-T07 Single instance。
- E01-T08 Window state。
- E01-T09 local assets / no CDN。
- E01-T10 ZIP release build。
- E01-T11 clean VM smoke test。
- E01-T12 no-console release configuration。

DoD：

- 解压可运行。
- 不需要 npm serve。
- 不需要终端。
- 不写业务 AppData。
- 关闭正常退出。

依赖：

E00 可并行部分进行。

---

## E02 — Lossless XML Core

任务：

- E02-T01 Encoding detector。
- E02-T02 UTF-16 decoder/encoder。
- E02-T03 Lossless XML node model。
- E02-T04 quick-xml parser adapter。
- E02-T05 writer。
- E02-T06 Canonical mapper。
- E02-T07 XmlBinding。
- E02-T08 unknown attr preservation。
- E02-T09 unknown element preservation。
- E02-T10 comments type protection。
- E02-T11 task tree mutation。
- E02-T12 TaskId allocator。
- E02-T13 semantic validator。
- E02-T14 golden tests。
- E02-T15 malformed document errors。

DoD：

- 所有 E00 fixture round-trip 通过。
- 单字段修改不删除无关数据。
- 编码保持。

依赖：

E00。

---

## E03 — Safe Persistence & Recovery

任务：

- E03-T01 fingerprint。
- E03-T02 temp write。
- E03-T03 validation before replace。
- E03-T04 Windows atomic replace。
- E03-T05 backup/recovery snapshot。
- E03-T06 save state machine。
- E03-T07 save error UI protocol。
- E03-T08 recovery journal。
- E03-T09 startup recovery。
- E03-T10 fault injection。
- E03-T11 disk full test。
- E03-T12 permission failure test。

DoD：

任意模拟 save failure 不损坏上一个有效文件。

依赖：

E02。

---

## E04 — SQLite Index & Workspace

任务：

- E04-T01 rusqlite layer。
- E04-T02 migrations。
- E04-T03 schema。
- E04-T04 workspace registry。
- E04-T05 workspace scan。
- E04-T06 document registry。
- E04-T07 task index。
- E04-T08 FTS。
- E04-T09 index delta update。
- E04-T10 rebuild index。
- E04-T11 corruption recovery。
- E04-T12 managed/linked document。
- E04-T13 relative portable paths。
- E04-T14 missing workspace relocation。

DoD：

删除 index.db 后可完全重建业务索引。

依赖：

E02，可和 E03 部分并行。

---

## E05 — File Watch & Conflict

任务：

- E05-T01 notify watcher。
- E05-T02 debounce。
- E05-T03 self-write detection。
- E05-T04 fingerprint verification。
- E05-T05 clean reload。
- E05-T06 dirty conflict state。
- E05-T07 conflict UI。
- E05-T08 UNC/polling fallback evaluation。

DoD：

外部修改永不被静默覆盖。

依赖：

E03/E04。

---

## E06 — Core Task UI

任务：

- E06-T01 三栏 layout。
- E06-T02 Sidebar。
- E06-T03 Task row DTO。
- E06-T04 flatten tree。
- E06-T05 expand/collapse。
- E06-T06 Inspector。
- E06-T07 title edit。
- E06-T08 priority/status/progress。
- E06-T09 start/due。
- E06-T10 tags/categories。
- E06-T11 create/delete。
- E06-T12 same-document move。
- E06-T13 keyboard navigation。
- E06-T14 multi-select。
- E06-T15 filtering without cloning。
- E06-T16 save indicator。

DoD：

达到现版本全部核心任务能力，同时消除当前副本编辑问题。

依赖：

E02/E03。

---

## E07 — Undo / Redo

任务：

- E07-T01 command abstraction。
- E07-T02 edit commands。
- E07-T03 tree commands。
- E07-T04 command coalescing。
- E07-T05 redo stack。
- E07-T06 save boundary behavior。
- E07-T07 crash recovery integration。

依赖：

E06 + E03。

---

## E08 — Participants / Dependencies / Progress Links

任务：

- E08-T01 participants domain。
- E08-T02 participant index。
- E08-T03 participant UI。
- E08-T04 dependency domain。
- E08-T05 dependency resolver。
- E08-T06 cycle detection。
- E08-T07 incoming index。
- E08-T08 dependency UI。
- E08-T09 progress link storage。
- E08-T10 URL validation。
- E08-T11 provider detection。
- E08-T12 open browser。

依赖：

E00 schema audit + E04 + E06。

---

## E09 — Attachments

任务：

- E09-T01 attachment model。
- E09-T02 managed attachment copy。
- E09-T03 linked file。
- E09-T04 URL attachment。
- E09-T05 relative path。
- E09-T06 open/reveal。
- E09-T07 missing attachment UI。
- E09-T08 attachment hash。
- E09-T09 delete/orphan policy。
- E09-T10 XML mapping。

依赖：

E00 + E03 + E06。

---

## E10 — Rich Text

任务：

- E10-T01 Tiptap minimal editor。
- E10-T02 toolbar。
- E10-T03 sanitizer。
- E10-T04 plain text compatibility。
- E10-T05 HTML comment compatibility。
- E10-T06 unsupported comment read-only。
- E10-T07 clipboard image。
- E10-T08 image asset import。
- E10-T09 relative image URL。
- E10-T10 orphan image GC。
- E10-T11 rich text round-trip fixtures。

依赖：

E00 + E09 + E02。

---

## E11 — Cross-document Transfer

任务：

- E11-T01 TransferManifest。
- E11-T02 ID mapping。
- E11-T03 subtree clone。
- E11-T04 internal dependency rewrite。
- E11-T05 external dependency policy。
- E11-T06 attachment transfer。
- E11-T07 target staging。
- E11-T08 target commit。
- E11-T09 source delete commit。
- E11-T10 transaction journal。
- E11-T11 startup transaction recovery。
- E11-T12 transfer undo。
- E11-T13 drag task to document UI。
- E11-T14 Copy/Move command UI。
- E11-T15 failure injection matrix。

DoD：

任意阶段强制终止后不会丢任务。

依赖：

E03/E08/E09。

---

## E12 — Search / Smart Views / Productivity

任务：

- E12-T01 global search。
- E12-T02 command palette。
- E12-T03 Today。
- E12-T04 Upcoming。
- E12-T05 Overdue。
- E12-T06 Unscheduled。
- E12-T07 filter model。
- E12-T08 saved view。
- E12-T09 quick add parser。
- E12-T10 local shortcuts。
- E12-T11 optional global quick add。

依赖：

E04/E06。

---

## E13 — File Library UX

任务：

- E13-T01 new document without Save As。
- E13-T02 rename。
- E13-T03 move document。
- E13-T04 duplicate document。
- E13-T05 import managed。
- E13-T06 link external。
- E13-T07 remove reference。
- E13-T08 trash。
- E13-T09 restore trash。
- E13-T10 reveal in Explorer。

依赖：

E03/E04。

---

## E14 — Performance & Large Workspace

任务：

- E14-T01 benchmark generator。
- E14-T02 1k fixture。
- E14-T03 10k fixture。
- E14-T04 50k fixture。
- E14-T05 startup profiling。
- E14-T06 parse profiling。
- E14-T07 search profiling。
- E14-T08 task render profiling。
- E14-T09 save profiling。
- E14-T10 memory profiling。
- E14-T11 decide virtualization based on evidence。

依赖：

核心功能稳定后。

---

## E15 — Portable Release Hardening

任务：

- E15-T01 release ZIP。
- E15-T02 license bundle。
- E15-T03 WebView2 runtime detection。
- E15-T04 no-network test。
- E15-T05 clean Windows 10。
- E15-T06 clean Windows 11。
- E15-T07 removable drive test。
- E15-T08 relative path relocation test。
- E15-T09 read-only directory behavior。
- E15-T10 single instance open-file flow。
- E15-T11 crash recovery release test。
- E15-T12 version migration test。

---

# 51. 建议里程碑依赖

```text
M0  Compatibility & Test Corpus
 │
 ├─────────────┐
 ↓             ↓
M1 Desktop     M2 XML Core
 │             │
 └──────┬──────┘
        ↓
M3 Safe Persistence + Workspace + SQLite
        ↓
M4 Core Task UX
        ↓
M5 Participants / Dependency / Attachment
        ↓
M6 Rich Text
        ↓
M7 Cross-document Transfer
        ↓
M8 Search / Smart Views / Productivity
        ↓
M9 Performance / Portable Release Hardening
```

其中：

- E03 Safe Persistence 不能被后续功能绕过。
- E11 Cross-document Transfer 在 Dependency/Attachment 之前做，会导致返工。
- Rich Text 必须等 Comments compatibility 有明确结论。
- Calendar/Kanban 不进入上述主链路。

---

# 52. 每个 Issue 的标准模板

后续拆 Linear/GitHub Issue 时，每个实现任务至少包含：

```text
Title

Context
- 为什么存在

Scope
- 本任务做什么

Out of Scope
- 明确不做什么

Architecture
- 所属模块
- 是否涉及 XML/SQLite/FS

Data Safety
- 是否改变业务数据
- 失败时怎么恢复

Acceptance Criteria
- 可测条件

Tests
- Unit
- Integration
- Fixture
- E2E

Dependencies
- blocks / blocked by
```

涉及数据写入的 Issue 必须额外回答：

```text
如果进程在这个操作中间被 kill，会发生什么？
```

答不出来，不允许进入实现完成状态。

---

# 53. Definition of Done — 全局

功能不能仅以“UI 看起来能用”为完成。

必须满足：

1. Domain rule 完成。
2. XML mapping 完成。
3. Index update 完成。
4. Undo/Recovery 策略完成。
5. Error path 完成。
6. Unit test。
7. Integration test。
8. 需要时 Golden fixture。
9. 无静默数据丢失。
10. 无新增 CDN/runtime server。
11. Portable 模式通过。
12. 文档更新。

---

# 54. 推荐依赖与许可证策略

## Rust

### Tauri 2

使用官方生态。

### rusqlite

MIT；bundled SQLite 可减少机器依赖。

### quick-xml

MIT；适合作为高性能 XML reader/writer。

注意 UTF-16 必须由我们自己的 Encoding Layer 处理。

### notify

跨平台 watcher；Windows 使用 ReadDirectoryChangesW。

### blake3

用于文件 fingerprint/hash。

依赖引入前仍需要在 lockfile 阶段进行 license/security audit。

## Frontend

### Vue 3

核心 UI。

### Tiptap OSS

MIT Open Source Core。

只允许确认 MIT 的 OSS packages。

不得误用 Tiptap Pro template/extensions。

---

# 55. 技术调研依据

截至本文日期，以下能力已通过官方/一手资料确认：

1. Tauri 2 使用系统 WebView，并通过 IPC/Capabilities 区分 Rust Core 和 WebView 信任边界。  
   https://v2.tauri.app/security/  
   https://v2.tauri.app/concept/inter-process-communication/

2. Tauri 支持为 WebView 指定 data directory；Windows 下可以把 WebView2 UDF 放入 Portable `Data/webview2`。  
   https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindowBuilder.html

3. Microsoft WebView2 支持自定义 User Data Folder；UDF 包含 WebView 的缓存、IndexedDB 等浏览器数据。  
   https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder

4. Tauri Windows 默认使用系统 WebView2，Fixed Version 会显著增加分发体积，因此不适合作为默认轻量 Portable 包。  
   https://v2.tauri.app/distribute/windows-installer/

5. Tauri 官方提供 single-instance、global shortcut、window-state 等桌面插件能力。  
   https://v2.tauri.app/plugin/single-instance/  
   https://v2.tauri.app/plugin/global-shortcut/  
   https://v2.tauri.app/plugin/window-state/

6. `quick-xml` 当前是高性能 Rust XML reader/writer，MIT；但其文档明确说明 UTF-16 不由 parser 直接支持，所以本项目必须有独立 Encoding Layer。  
   https://docs.rs/quick-xml/latest/quick_xml/  
   https://docs.rs/crate/quick-xml/latest

7. `rusqlite`/`libsqlite3-sys` 为 MIT；bundled feature 可静态编译 SQLite。  
   https://docs.rs/crate/rusqlite/latest

8. `notify` Windows backend 使用 ReadDirectoryChangesW。  
   https://docs.rs/crate/notify/latest

9. Tiptap Editor Open Source Core 为 MIT，并基于 ProseMirror，适合模块化 Rich Text。  
   https://github.com/ueberdosis/tiptap  
   https://tiptap.dev/

10. AbstractSpoon ToDoList 的公开资源中可以确认 Allocated To、Categories、Priority、Risk、Status、Start Date、Time Estimate、Tags、Task ID 等成熟字段概念；具体 XML 表示仍由 E00 Compatibility Audit 固化。  
    https://github.com/abstractspoon/ToDoList_Resources  
    https://github.com/abstractspoon/ToDoList_Dev

---

# 56. 最终架构摘要

最终运行架构：

```text
┌─────────────────────────────────────────────┐
│            ModernToDoList.exe               │
│                                             │
│  ┌───────────────────────────────────────┐  │
│  │ Vue 3 / TypeScript UI                 │  │
│  │                                       │  │
│  │ Sidebar / Task Tree / Inspector       │  │
│  │ Search / Quick Add / Rich Text        │  │
│  └──────────────────┬────────────────────┘  │
│                     │ Typed IPC              │
│  ┌──────────────────▼────────────────────┐  │
│  │ Rust Application Core                │  │
│  │                                      │  │
│  │ Workspace Service                    │  │
│  │ Document Service                     │  │
│  │ Task Service                         │  │
│  │ Transfer Service                     │  │
│  │ Attachment Service                   │  │
│  │ Dependency Service                   │  │
│  │ Save / Recovery Service              │  │
│  └───────────┬────────────┬─────────────┘  │
│              │            │                 │
│       ┌──────▼──────┐ ┌──▼──────────┐      │
│       │ Lossless XML│ │ SQLite Index│      │
│       └──────┬──────┘ └─────────────┘      │
│              │                              │
└──────────────┼──────────────────────────────┘
               │
         Windows File System
               │
        XML / TDL / Assets
               │
         SOURCE OF TRUTH
```

Portable：

```text
ModernToDoList/
├── ModernToDoList.exe
└── Data/
    ├── index.db
    ├── recovery/
    ├── cache/
    └── webview2/
```

Workspace：

```text
Workspace/
├── *.tdl / *.xml
├── Assets/
└── .moderntodo/
```

最重要的数据原则：

```text
删除 ModernToDoList 的索引数据库
           ≠
删除用户任务

只要 Workspace 中的 XML/TDL 和 Assets 存在，
ModernToDoList 就必须可以重新构建自己的全部索引并继续工作。
```

最重要的写入原则：

```text
任何写操作：
验证 → 暂存 → 验证输出 → 原子提交 → 更新索引

任何跨文件 Move：
复制 → 验证 Target → 提交 Target → 再删除 Source
```

最重要的产品原则：

> ModernToDoList 2.0 的目标不是成为功能数量最多的 To-do List，而是在保持 Portable、本地、轻量、开放数据的前提下，把任务管理、文件管理、跨文件组织、参与者、依赖、附件和富文本做到足够成熟，并把“用户数据绝不因应用实现细节而悄悄损坏”作为所有功能之上的最高优先级。

---

# 57. 后续文档关系

本文应作为顶层 Architecture Baseline。

后续建议从本文派生，而不是各自重新定义架构：

```text
ModernToDoList_2.0_Detailed_Technical_Architecture.md
        │
        ├─ ModernToDoList_2.0_Roadmap.md
        │
        ├─ ModernToDoList_2.0_Task_Backlog.md
        │
        ├─ TDL_FIELD_MAPPING.md
        │
        ├─ XML_COMPATIBILITY_TEST_PLAN.md
        │
        ├─ SQLITE_SCHEMA.md
        │
        ├─ IPC_CONTRACT.md
        │
        └─ UI_UX_SPEC.md
```

Roadmap 和 Issue 拆分必须引用本文 Epic/Invariant/DoD，不得另起一套不兼容的数据和技术设计。
