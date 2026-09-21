# ModernToDoList 2.0 — 完整开发路线图与阶段性测试回归标准

> 文档定位：基于《ModernToDoList 2.0 — 产品与详细技术架构设计书》的执行级 Roadmap，可直接继续拆分为 Linear/GitHub Issue、开发迭代和 Milestone。  
> 目标版本：ModernToDoList 2.0 GA  
> 平台：Windows 10/11 x64，Portable First  
> 技术基线：Tauri 2 + Vue 3 + TypeScript + Rust + SQLite + Lossless XML/TDL  
> 文档日期：2026-09-13  
> 说明：本文不提供未经团队规模验证的工期承诺；所有阶段以**依赖关系、可验证成果和质量 Gate**推进。确定团队人数后，可在不改变依赖图的前提下映射为 Sprint/日期计划。

---

# 1. Roadmap 总目标

ModernToDoList 2.0 的开发不是“把当前网页套进 EXE”，而是完成一次受控的产品与数据架构升级：

```text
当前版本
Browser / Local Web
      ↓
数据兼容和安全底座
      ↓
Portable Windows Desktop Core
      ↓
Workspace / SQLite Index / File Watch
      ↓
现代任务管理 UI
      ↓
参与者 / 依赖 / 附件 / 富文本
      ↓
跨文件安全迁移
      ↓
搜索 / Smart Views / Quick Add
      ↓
性能和 Portable Release Hardening
      ↓
ModernToDoList 2.0 GA
```

整个 Roadmap 的最高优先级不是功能数量，而是三个不可退让条件：

1. **数据完整保真**：任何编辑不得静默删除或转换未知 XML/TDL 数据。
2. **故障安全**：任何保存、删除、跨文件移动在崩溃、磁盘错误、权限错误下都必须优先保证旧数据仍然存在。
3. **Portable 真正成立**：无需安装、无需后台服务、无需终端、无需管理员权限，程序数据默认位于程序目录并可整体搬迁。

---

# 2. Roadmap 执行规则

## 2.1 阶段不是“功能完成”，而是可发布能力完成

每一个 Milestone 必须同时满足：

- 功能实现完成；
- 单元测试完成；
- 集成测试完成；
- 对应历史回归完成；
- 数据安全场景完成；
- 文档更新完成；
- 明确的 Exit Gate 全部通过。

禁止：

```text
UI 能点
=
任务完成
```

必须是：

```text
功能
+ 数据模型
+ 持久化
+ 错误路径
+ 回滚/恢复
+ 测试
+ 回归
+ 文档
=
任务完成
```

## 2.2 测试失败优先级高于后续功能

如果某阶段发现：

- XML round-trip 丢字段；
- 保存可能破坏旧文件；
- Move 有任务丢失路径；
- SQLite 与 XML 形成双真相；
- Portable 产生不可控 AppData/后台服务；
- 外部修改可能被静默覆盖；

则：

> 当前 Milestone 不得关闭，后续依赖该能力的阶段不得进入完成状态。

## 2.3 所有高风险写入任务必须回答

每个涉及写入的 Issue 必须在 PR 描述中回答：

```text
如果程序在该操作执行到任意中间步骤时被强制 Kill，
用户最坏会得到什么状态？
能否自动恢复？
如果不能自动恢复，是否至少保证原始有效数据仍存在？
```

## 2.4 Roadmap 中的测试任务是正式工作项

测试不是开发任务末尾的隐含动作。

每个 Milestone 都有独立 `QA-Mx-*` 任务，必须进入 Issue Tracker，并且拥有：

- Owner；
- 输入 Fixture；
- 执行步骤；
- Expected Result；
- 回归范围；
- 失败处理。

---

# 3. 阶段总览

| 阶段 | Milestone | 核心成果 | 主要架构 Epic |
|---|---|---|---|
| M0 | Compatibility Baseline Locked | 冻结真实 XML/TDL 兼容契约和测试语料 | E00 |
| M1 | Portable Desktop Shell | 真正无需 Web Server 的 Windows Portable 桌面壳 | E01 |
| M2 | Lossless XML Core | 可证明无损的 XML/TDL 读写核心 | E02 |
| M3 | Data Safety Core | Atomic Save、Recovery、Undo 基础和故障安全 | E03 + E07 基础 |
| M4 | Workspace Data Platform | Workspace、SQLite 索引、文件监控、本地文件管理底座 | E04 + E05 |
| M5 | Core Product Alpha | 现有核心任务能力全部迁移到新架构并达到可日用 Alpha | E06 + E07 |
| M6 | Task Relations Beta | 参与者、依赖、Progress Link、附件达到完整可用 | E08 + E09 |
| M7 | Rich Content Beta | 富文本描述、图片资产和内容兼容完整落地 | E10 |
| M8 | Multi-Document Beta | 跨文件 Copy/Move、文件 Library、Trash 达到故障安全 | E11 + E13 |
| M9 | Productivity Feature Complete | 搜索、Smart Views、Saved Views、Quick Add、Command Palette 完成 | E12 |
| M10 | 2.0 Release Candidate → GA | 性能、Portable、兼容、稳定性和发布回归全部通过 | E14 + E15 |

---

# 4. Critical Path

```text
M0 Compatibility Baseline
        ↓
M2 Lossless XML Core
        ↓
M3 Data Safety Core
        ↓
M4 Workspace Data Platform
        ↓
M5 Core Product Alpha
        ↓
M6 Task Relations Beta
        ↓
M7 Rich Content Beta
        ↓
M8 Multi-Document Beta
        ↓
M9 Productivity Feature Complete
        ↓
M10 Release Candidate / GA
```

M1 Portable Desktop Shell 可以与 M0 后半段部分并行，但 M1 不能绕过 M0/M2 开始实现业务数据写入。

可以并行的典型工作：

```text
M0 Compatibility
    ├── M1 Desktop Skeleton
    └── UI Design System Prototype

M3 Safe Persistence
    ├── M4 SQLite Infrastructure
    └── M5 UI Shell Prototype（只接 Mock DTO）

M6 Attachments
    └── M7 Rich Text Toolbar Prototype
```

禁止提前并行：

```text
Cross-document Move
before
Dependency + Attachment + Transaction Core

Rich Text Write
before
Comments Compatibility Contract

Full Task UI direct XML write
before
Lossless XML Core
```

---

# 5. 全局质量等级

定义统一测试等级：

## Q0 — Compile / Static

- TypeScript typecheck。
- Vue compile。
- Rust build。
- rustfmt。
- clippy。
- lint。
- license audit。

## Q1 — Unit

针对纯函数/模块：

- parser；
- ID allocator；
- path resolver；
- dependency graph；
- filter；
- command；
- DTO validation。

## Q2 — Integration

真实模块组合：

- XML + Save Service；
- SQLite + Workspace；
- Attachment + XML；
- Dependency + Transfer；
- UI IPC + Rust Core。

## Q3 — Compatibility / Golden

必须使用真实 XML/TDL Fixtures 做：

- no-op；
- single-field edit；
- tree edit；
- unknown fields；
- encoding；
- comments type；
- AbstractSpoon interoperability。

## Q4 — Fault Injection

强制模拟：

- process kill；
- write error；
- disk full；
- permission denied；
- target disappeared；
- DB locked/corrupt；
- external file modified。

## Q5 — UI E2E

从用户行为验证：

- 打开；
- 创建；
- 编辑；
- 拖拽；
- 搜索；
- Move；
- Rich Text；
- Recovery。

## Q6 — Performance

固定 fixture：

- 1k；
- 10k；
- 50k；
- 100k stress。

## Q7 — Portable / Clean Machine

Windows 10/11 VM：

- 无 Node；
- 无 Rust；
- 无安装；
- 无网络；
- 无管理员权限；
- 可移动目录。

每个 Milestone 在自己的 Exit Gate 中会明确要求达到哪些质量等级。

---

# 6. M0 — Compatibility Baseline Locked

## 6.1 阶段目标

在开始任何新数据写入实现之前，把“我们到底需要兼容什么 XML/TDL”从推测变成可执行契约。

对应架构 Epic：

```text
E00 — Baseline Freeze & Compatibility Corpus
```

## 6.2 Milestone 成果

Milestone 名称：

> **M0 — Compatibility Baseline Locked**

完成后必须拥有：

```text
docs/compatibility/
├── TDL_FIELD_MAPPING.md
├── COMMENT_FORMAT_MATRIX.md
├── DEPENDENCY_FORMAT.md
├── FILELINK_FORMAT.md
├── ID_ALLOCATION_RULES.md
└── COMPATIBILITY_POLICY.md

tests/fixtures/xml/
├── canonical/
├── encoding/
├── comments/
├── dependencies/
├── attachments/
├── unknown/
├── malformed/
└── real-world/
```

这是后续所有数据层开发的测试契约。

## 6.3 开发任务

| ID | 任务 | 输出 | 前置 |
|---|---|---|---|
| RD-M0-001 | 冻结当前 ModernToDoList baseline commit | `BASELINE.md` | 无 |
| RD-M0-002 | 收集当前应用能够生成和读取的 XML 样本 | current fixtures | 001 |
| RD-M0-003 | 收集真实 AbstractSpoon TDL/XML 文件样本 | real-world fixtures | 无 |
| RD-M0-004 | 建立 UTF-8/UTF-8 BOM/UTF-16LE/UTF-16BE 样本 | encoding fixtures | 无 |
| RD-M0-005 | 盘点所有任务基础字段 | field inventory | 003 |
| RD-M0-006 | 确认 TITLE/PERCENTDONE/PRIORITY/RISK/STATUS 映射 | field mapping | 005 |
| RD-M0-007 | 确认 Start/Due/Completed 日期字段及格式 | date mapping | 005 |
| RD-M0-008 | 确认 Category/Tag 表示 | tag mapping | 005 |
| RD-M0-009 | 确认 Allocated To/By 和多人参与者表示 | participant mapping | 005 |
| RD-M0-010 | 确认 File Link 单个/多个链接表示 | filelink mapping | 005 |
| RD-M0-011 | 确认 Dependency 本文件和跨文件表示 | dependency mapping | 005 |
| RD-M0-012 | 确认 COMMENTSTYPE 和内容载荷关系 | comment matrix | 003 |
| RD-M0-013 | 确认 Plain Text Comments round-trip | fixtures | 012 |
| RD-M0-014 | 确认 HTML Comments round-trip | fixtures | 012 |
| RD-M0-015 | 确认 Markdown/RTF/其他 Comments 行为 | matrix | 012 |
| RD-M0-016 | 确认 Custom Attributes 格式和保留规则 | mapping | 003 |
| RD-M0-017 | 确认任务 ID 的允许格式和分配语义 | ID rules | 003 |
| RD-M0-018 | 确认未知 XML node/attribute 的真实样本 | unknown fixtures | 003 |
| RD-M0-019 | 建立超深树/大量 sibling/空任务等边界样本 | edge fixtures | 005 |
| RD-M0-020 | 建立损坏/截断/非法 encoding 样本 | malformed fixtures | 004 |
| RD-M0-021 | 定义 Semantic Lossless 的精确验收规则 | compatibility policy | 006-020 |
| RD-M0-022 | 定义哪些字段属于 Tier A/B/C | tier matrix | 021 |
| RD-M0-023 | 定义 ModernToDoList 扩展字段命名空间策略 | extension policy | 016 |
| RD-M0-024 | 定义未来 schema audit 新增 fixture 流程 | contribution guide | 021 |

## 6.4 测试与回归任务

| ID | 测试任务 | 通过标准 |
|---|---|---|
| QA-M0-001 | Fixture 可解析性检查 | 所有“有效 fixture”能被至少一个参考解析器读取 |
| QA-M0-002 | Encoding fixture 字节级验证 | BOM、声明、实际编码匹配预期 |
| QA-M0-003 | Unknown data fixture 验证 | 样本确实包含未知 attr/node |
| QA-M0-004 | Comments matrix 人工核对 | 每种类型有明确 Read/Write/Read-only 策略 |
| QA-M0-005 | Participant mapping 核对 | 多参与者样本不依赖推测 |
| QA-M0-006 | Dependency mapping 核对 | 本地/外部/无法解析三种行为均定义 |
| QA-M0-007 | FileLink mapping 核对 | 多链接、相对路径、绝对路径均覆盖 |
| QA-M0-008 | Baseline regression capture | 当前版本已知正确和已知错误行为均记录 |
| QA-M0-009 | Compatibility policy review | 所有后续开发者能据文档判断字段是否可写 |
| QA-M0-010 | Fixture immutable hash manifest | fixture 建立 hash，避免测试语料被无意修改 |

## 6.5 Exit Gate

必须全部满足：

- `TDL_FIELD_MAPPING.md` 不存在关键 TBD；
- P0 字段全部有真实样本；
- Encoding/Comments/Dependency/FileLink/Custom Attribute 均有测试语料；
- 明确哪些 Comments 类型 2.0 第一版只能只读；
- ID 分配策略有依据；
- Semantic Lossless 验收定义完成。

阻断下一阶段的失败：

- 仍然不知道 HTML/RTF 的实际表示；
- 仍然假设参与者/依赖 XML 结构；
- 没有 UTF-16 样本；
- 没有未知字段保留样本。

---

# 7. M1 — Portable Desktop Shell

## 7.1 阶段目标

完成真正的 Windows Portable 桌面运行基座，但此阶段不承诺完整业务功能。

对应：

```text
E01 — Windows Portable Foundation
```

## 7.2 Milestone 成果

> **M1 — Portable Desktop Shell**

用户可以：

```text
解压 ZIP
→ 双击 ModernToDoList.exe
→ 正常打开窗口
→ 不出现终端
→ 不需要 localhost
→ 关闭程序后退出
```

## 7.3 开发任务

| ID | 任务 | 输出 | 前置 |
|---|---|---|---|
| RD-M1-001 | 初始化 Vue 3 + TypeScript + Vite | frontend skeleton | M0 可并行 |
| RD-M1-002 | 初始化 Tauri 2 | desktop skeleton | 001 |
| RD-M1-003 | 建立 Rust Core crate/module | core skeleton | 002 |
| RD-M1-004 | 建立 typed IPC 最小链路 | ping/runtime DTO | 003 |
| RD-M1-005 | 移除 Vue/Tailwind/Icon CDN | offline assets | 001 |
| RD-M1-006 | 建立 production CSP | CSP policy | 005 |
| RD-M1-007 | 获取 EXE 目录并解析 Portable Root | portable resolver | 003 |
| RD-M1-008 | `Data/` 自动创建与可写性检查 | Data bootstrap | 007 |
| RD-M1-009 | 显式设置 WebView2 UDF 至 `Data/webview2` | portable UDF | 008 |
| RD-M1-010 | 建立 settings.json 基础加载/保存 | settings core | 008 |
| RD-M1-011 | Single Instance | single-instance | 002 |
| RD-M1-012 | 第二实例参数转交第一实例 | open arg transport | 011 |
| RD-M1-013 | Window State 本地保存 | window-state | 010 |
| RD-M1-014 | Release build 不显示 console | Windows release config | 002 |
| RD-M1-015 | ZIP packaging script | release zip | 014 |
| RD-M1-016 | WebView2 runtime detection | runtime diagnostics | 002 |
| RD-M1-017 | Portable path 搬迁测试辅助 | relocation harness | 007 |
| RD-M1-018 | 建立 LICENSES 生成流程 | legal artifacts | 015 |
| RD-M1-019 | 建立应用版本信息 | version metadata | 015 |
| RD-M1-020 | 建立 Crash/Panic 顶层日志目录 | bootstrap logging | 008 |

## 7.4 测试与回归任务

| ID | 场景 | 通过标准 |
|---|---|---|
| QA-M1-001 | Windows 11 clean VM | 无 Node/Rust 也能启动 |
| QA-M1-002 | Windows 10 clean VM | 满足支持基线时正常启动 |
| QA-M1-003 | 断网启动 | UI 完整加载，无 CDN 请求失败 |
| QA-M1-004 | 程序目录整体搬迁 | Data 随程序搬迁并可继续启动 |
| QA-M1-005 | Data 不可写 | 明确错误，不静默写 AppData |
| QA-M1-006 | 双开 | 第二实例不形成第二套编辑进程 |
| QA-M1-007 | 关闭窗口 | 默认真正退出，无 ModernToDoList 常驻 |
| QA-M1-008 | 无终端窗口 | Release EXE 双击不出现 console |
| QA-M1-009 | WebView2 UDF | 浏览器数据实际位于 `Data/webview2` |
| QA-M1-010 | no-CDN network capture | 启动不访问 Vue/Tailwind/Icon CDN |
| QA-M1-011 | 删除 Data/cache | 下次启动自动重建，不影响程序 |
| QA-M1-012 | ZIP 解压路径包含中文/空格 | 可正常运行 |

## 7.5 Exit Gate

- Portable Clean Machine Q7 通过；
- 不依赖 localhost；
- 不需要终端；
- 不需要安装；
- 不需要管理员权限；
- 默认业务配置不写 AppData；
- WebView2 UDF 明确位于 Data；
- no-network 可启动。

---

# 8. M2 — Lossless XML Core

## 8.1 阶段目标

建立后续所有功能依赖的数据核心。

对应：

```text
E02 — Lossless XML Core
```

这是整个项目第一优先级质量阶段。

## 8.2 Milestone 成果

> **M2 — Lossless XML Core**

交付一个不依赖 UI 的 Rust Core：

```text
bytes
  ↓
Encoding Layer
  ↓
Lossless XML Tree
  ↓
Canonical Task Model
  ↓
Patch
  ↓
Lossless XML Writer
  ↓
bytes
```

## 8.3 开发任务

| ID | 任务 | 验收重点 |
|---|---|---|
| RD-M2-001 | 定义 XmlEncodingMeta | encoding/BOM/line ending 可保存 |
| RD-M2-002 | 实现 BOM detector | UTF-8/16 LE/BE |
| RD-M2-003 | 实现 XML declaration detector | 不依赖完整 parser |
| RD-M2-004 | 实现 UTF-16 → internal UTF-8 解码 | 无 replacement corruption |
| RD-M2-005 | 实现 internal UTF-8 → 原 encoding 写出 | 保持 encoding |
| RD-M2-006 | 集成 quick-xml tokenizer | 所有 M0 有效 fixture 可读取 |
| RD-M2-007 | Lossless XmlNode 模型 | Element/Text/CData/Comment/PI |
| RD-M2-008 | 保留未知 attribute | 不被 mapper 丢弃 |
| RD-M2-009 | 保留未知 element | 不被 mapper 丢弃 |
| RD-M2-010 | 保留 task node order | tree order 正确 |
| RD-M2-011 | 定义 DocumentMetadata | encoding/source/fingerprint placeholders |
| RD-M2-012 | 定义 Domain ID newtypes | WorkspaceId/DocumentId/TaskId/TaskKey |
| RD-M2-013 | 实现基础 Task mapper | title/progress/priority/status |
| RD-M2-014 | 实现日期 mapper | start/due/completed |
| RD-M2-015 | 实现 category/tag mapper | 多值保持 |
| RD-M2-016 | 实现 participant mapper | 按 M0 契约 |
| RD-M2-017 | 实现 file link mapper | 按 M0 契约 |
| RD-M2-018 | 实现 dependency mapper | raw unresolved 保留 |
| RD-M2-019 | 实现 Comments mapper | type + raw payload |
| RD-M2-020 | 实现 Custom Attribute 保留 | unknown custom 不丢 |
| RD-M2-021 | XmlBinding：field → source XML | patch 定位 |
| RD-M2-022 | Task tree add | 新 node 合法 |
| RD-M2-023 | Task tree delete | 子 node 真删除 |
| RD-M2-024 | Task tree reorder | sibling 顺序正确 |
| RD-M2-025 | Task tree reparent | 不残留旧子节点 |
| RD-M2-026 | Task ID allocator | 按 document policy |
| RD-M2-027 | Semantic validator | 保存前验证 |
| RD-M2-028 | Parse error model | UI 可理解错误代码 |
| RD-M2-029 | Unsupported comment type protection | 不允许静默转换 |
| RD-M2-030 | 编写 XML Core API 文档 | 后续 service 唯一入口 |

## 8.4 测试与回归任务

| ID | 测试 | Gate |
|---|---|---|
| QA-M2-001 | 全部 M0 fixture parse | 100% 有效 fixture 成功或符合策略 |
| QA-M2-002 | UTF-8 no BOM round-trip | encoding 保持 |
| QA-M2-003 | UTF-8 BOM round-trip | BOM 保持 |
| QA-M2-004 | UTF-16LE round-trip | 编码不被改写 |
| QA-M2-005 | UTF-16BE round-trip | 编码不被改写 |
| QA-M2-006 | Unknown attr preservation | 修改 Title 后 unknown attr 仍在 |
| QA-M2-007 | Unknown node preservation | 修改 DueDate 后 unknown node 仍在 |
| QA-M2-008 | Nested child delete regression | 删除后重新解析不复活 |
| QA-M2-009 | Reparent regression | 节点只存在新位置 |
| QA-M2-010 | Participant round-trip | 值完整 |
| QA-M2-011 | Dependency unresolved round-trip | raw reference 保留 |
| QA-M2-012 | FileLink round-trip | 路径/多个链接保持 |
| QA-M2-013 | Plain Comments | 可编辑且正确写回 |
| QA-M2-014 | HTML Comments | 只有已验证格式允许写 |
| QA-M2-015 | Unsupported Comments | 保存无关字段不改变 comment payload |
| QA-M2-016 | Custom Attribute | 未理解字段保持 |
| QA-M2-017 | malformed XML | 拒绝打开为可编辑文档，不崩溃 |
| QA-M2-018 | Golden semantic diff | 只允许预期字段变化 |
| QA-M2-019 | Deep tree | 栈/递归策略不崩溃 |
| QA-M2-020 | Current-version regression fixtures | 当前已有功能 XML 可读 |

## 8.5 Exit Gate

必须达到：

- Q1/Q2/Q3 全通过；
- 所有 P0 fixture 有自动化测试；
- 任何单字段修改不会删除未知数据；
- Nested delete/reparent bug 被测试锁定；
- Comments 类型保护生效；
- UTF-16 真正按字节正确写出；
- XML Core API 不依赖 Vue/Tauri。

M2 未通过时，不允许后续功能直接写 XML。

---

# 9. M3 — Data Safety Core

## 9.1 阶段目标

把“能写 XML”升级成“任何合理故障条件下都不会损坏上一份有效数据”。

对应：

```text
E03 — Safe Persistence & Recovery
E07 — Undo/Redo 基础
```

## 9.2 Milestone 成果

> **M3 — Data Safety Core**

实现：

```text
Safe Save
Recovery Journal
Fingerprint
Session Revision
Command Undo foundation
```

## 9.3 开发任务

| ID | 任务 | 验收重点 |
|---|---|---|
| RD-M3-001 | FileFingerprint 结构 | size/mtime/hash |
| RD-M3-002 | BLAKE3 hash service | streaming |
| RD-M3-003 | DocumentSession | canonical+xml+revision |
| RD-M3-004 | Dirty/saved revision model | 不依赖 UI |
| RD-M3-005 | per-document mutation lock | 防并发写 |
| RD-M3-006 | Save state machine | explicit phases |
| RD-M3-007 | 同目录 temp writer | 不直接覆盖 |
| RD-M3-008 | flush/sync temp | 尽量持久化 |
| RD-M3-009 | temp reopen validation | parse + semantic validate |
| RD-M3-010 | Windows atomic replacement | 原文件安全替换 |
| RD-M3-011 | save generation/self fingerprint | watcher 后续使用 |
| RD-M3-012 | Recovery snapshot | 保存前关键恢复点 |
| RD-M3-013 | Recovery journal format | phase 可恢复 |
| RD-M3-014 | startup incomplete save recovery | 自动/人工决策 |
| RD-M3-015 | Save error code model | permission/disk/io |
| RD-M3-016 | `Ctrl+S` immediate flush backend | 强制保存 |
| RD-M3-017 | autosave debounce coordinator | 文档级 |
| RD-M3-018 | revision conflict check | stale UI request rejected |
| RD-M3-019 | UndoableCommand trait | core |
| RD-M3-020 | UpdateField command | base command |
| RD-M3-021 | Add/Delete/Move basic commands | tree commands |
| RD-M3-022 | command coalescing | 连续文本输入 |
| RD-M3-023 | redo stack | 正确失效规则 |
| RD-M3-024 | Recovery Center core DTO | UI 后接 |
| RD-M3-025 | 数据安全事件日志 | 不记录正文 |

## 9.4 测试与回归任务

| ID | 故障场景 | 通过标准 |
|---|---|---|
| QA-M3-001 | 正常 Atomic Save | 保存后可重新解析 |
| QA-M3-002 | temp 写入前 kill | 原文件完整 |
| QA-M3-003 | temp 写到一半 kill | 原文件完整 |
| QA-M3-004 | temp 写完未 validate kill | 原文件完整 |
| QA-M3-005 | validate 后 replace 前 kill | 原文件完整 |
| QA-M3-006 | replace 异常 | 至少保留一份有效版本 |
| QA-M3-007 | disk full | 原文件不被 truncate |
| QA-M3-008 | permission denied | session 保持 dirty，可另存 |
| QA-M3-009 | stale revision request | 返回 RevisionConflict，不覆盖 |
| QA-M3-010 | Undo title edit | 恢复 XML/domain |
| QA-M3-011 | Undo nested delete | 完整子树恢复 |
| QA-M3-012 | Redo after undo | 状态一致 |
| QA-M3-013 | 新修改后 redo invalidation | redo 清空 |
| QA-M3-014 | text command coalescing | 多次按键一次撤销 |
| QA-M3-015 | crash journal startup | 可识别未完成操作 |
| QA-M3-016 | recovery data 不等于 source of truth | 删除 recovery 不损业务 |
| QA-M3-017 | 保存后 reopen | XML/Canonical 一致 |
| QA-M3-018 | XML Golden 全回归 | M2 全套继续通过 |

## 9.5 Exit Gate

- Q4 Fault Injection 首次成为强制 Gate；
- 所有保存失败测试保证旧文件有效；
- Undo 不再使用整文档每按键快照；
- Recovery Journal 可以识别中断保存；
- Revision 冲突不能静默写入；
- M2 Golden 全量回归无退化。

---

# 10. M4 — Workspace Data Platform

## 10.1 阶段目标

建立现代本地数据管理方式：

```text
Workspace
+ Managed/Linked Documents
+ SQLite Index
+ File Watch
+ Rebuild
```

对应：

```text
E04
E05
```

## 10.2 Milestone 成果

> **M4 — Workspace Data Platform**

用户不再以“每次打开一个 XML 文件”为主流程，而是：

```text
打开 Workspace
→ 自动发现文档
→ 建立索引
→ 下次直接恢复
```

## 10.3 开发任务：SQLite

| ID | 任务 |
|---|---|
| RD-M4-001 | 引入 rusqlite bundled |
| RD-M4-002 | SQLite connection manager |
| RD-M4-003 | migration runner |
| RD-M4-004 | `schema_migrations` |
| RD-M4-005 | `workspaces` |
| RD-M4-006 | `documents` |
| RD-M4-007 | `task_index` |
| RD-M4-008 | `task_tags` |
| RD-M4-009 | `task_participants` |
| RD-M4-010 | `task_dependencies` |
| RD-M4-011 | `attachments_index` |
| RD-M4-012 | `progress_links_index` |
| RD-M4-013 | `saved_views` |
| RD-M4-014 | `ui_state` |
| RD-M4-015 | `recovery_records` |
| RD-M4-016 | SQLite WAL/local disk policy |
| RD-M4-017 | UNC/network DB journal fallback |
| RD-M4-018 | DB unavailable fallback mode |

## 10.4 开发任务：Workspace

| ID | 任务 |
|---|---|
| RD-M4-019 | Workspace domain model |
| RD-M4-020 | workspace.json metadata |
| RD-M4-021 | create workspace |
| RD-M4-022 | open existing workspace |
| RD-M4-023 | recursive document scan |
| RD-M4-024 | supported file extension policy |
| RD-M4-025 | Managed Document |
| RD-M4-026 | Linked Document |
| RD-M4-027 | relative-to-exe workspace path |
| RD-M4-028 | absolute path fallback |
| RD-M4-029 | missing workspace relocation |
| RD-M4-030 | recent workspace registry |
| RD-M4-031 | document stable ID strategy |
| RD-M4-032 | new/changed/removed file detection |
| RD-M4-033 | incremental reindex |
| RD-M4-034 | full rebuild index |
| RD-M4-035 | index progress/cancel |
| RD-M4-036 | index corruption recovery |

## 10.5 开发任务：File Watch

| ID | 任务 |
|---|---|
| RD-M4-037 | notify watcher integration |
| RD-M4-038 | per-workspace watch registration |
| RD-M4-039 | linked-file watch |
| RD-M4-040 | event debounce/coalescing |
| RD-M4-041 | self-save recognition by fingerprint/generation |
| RD-M4-042 | external clean reload |
| RD-M4-043 | dirty-session conflict state |
| RD-M4-044 | conflict DTO |
| RD-M4-045 | watcher overflow/error recovery |
| RD-M4-046 | focus/save-before fingerprint fallback |

## 10.6 测试与回归任务

| ID | 场景 | 通过标准 |
|---|---|---|
| QA-M4-001 | 创建 Workspace | metadata 正确 |
| QA-M4-002 | 扫描嵌套目录 | 找到合法任务文档 |
| QA-M4-003 | Linked Document | 不移动原文件 |
| QA-M4-004 | Managed Document | 正确归属 workspace |
| QA-M4-005 | 删除 index.db | 重新扫描可完整重建 |
| QA-M4-006 | SQLite corruption | 自动进入重建/恢复，不影响 XML |
| QA-M4-007 | 程序整体搬迁+相对 Workspace | 自动恢复路径 |
| QA-M4-008 | 绝对 Workspace 丢失 | 提示重新定位 |
| QA-M4-009 | 外部编辑 clean document | 自动 reload |
| QA-M4-010 | 外部编辑 dirty document | 禁止覆盖，进入冲突 |
| QA-M4-011 | 自己保存 watcher event | 不误判外部冲突 |
| QA-M4-012 | 大量文件 watcher burst | 去抖后正确重检 |
| QA-M4-013 | UNC Workspace 基础 | 可打开/扫描；异常有明确错误 |
| QA-M4-014 | DB unavailable | XML 仍可直接访问 |
| QA-M4-015 | Index 与 XML 人为不一致 | XML 胜出并修复 index |
| QA-M4-016 | M2/M3 全回归 | 兼容与保存安全不退化 |

## 10.7 Exit Gate

必须证明：

```text
SQLite 全删
≠
业务数据丢失
```

同时：

- Workspace 可迁移；
- 外部修改不可静默覆盖；
- DB failure 不破坏 XML；
- File Watcher 不成为唯一正确性机制；
- Q1/Q2/Q3/Q4 全回归。

---

# 11. M5 — Core Product Alpha

## 11.1 阶段目标

把当前版本已有的高价值能力全部迁移到新架构，并完成现代三栏 UI，使新版本第一次达到“可以真实日常使用”的 Alpha。

对应：

```text
E06
E07
```

## 11.2 Milestone 成果

> **M5 — Core Product Alpha**

包含：

- Sidebar；
- Task Tree；
- Inspector；
- 新建/编辑/删除；
- Tree Move；
- Start/Due；
- Priority；
- Status/Progress；
- Tags；
- Search 基础入口；
- 自动保存状态；
- Undo/Redo；
- Keyboard Navigation。

## 11.3 UI/基础任务

| ID | 任务 |
|---|---|
| RD-M5-001 | 建立设计 token：spacing/type/radius |
| RD-M5-002 | 主窗口三栏布局 |
| RD-M5-003 | 可调 Sidebar 宽度 |
| RD-M5-004 | 可调 Inspector 宽度 |
| RD-M5-005 | 空状态 |
| RD-M5-006 | Loading 状态 |
| RD-M5-007 | Save status component |
| RD-M5-008 | Error notification/toast |
| RD-M5-009 | Confirmation dialog |
| RD-M5-010 | Keyboard focus style |
| RD-M5-011 | command registry 基础 |
| RD-M5-012 | IPC client typed wrapper |

## 11.4 Task Tree

| ID | 任务 |
|---|---|
| RD-M5-013 | TaskSummary DTO |
| RD-M5-014 | flattened visible rows |
| RD-M5-015 | expand/collapse |
| RD-M5-016 | selection |
| RD-M5-017 | selected task restore |
| RD-M5-018 | task completion |
| RD-M5-019 | priority indicator |
| RD-M5-020 | due/start indicator |
| RD-M5-021 | tag preview |
| RD-M5-022 | child count |
| RD-M5-023 | add root task |
| RD-M5-024 | add subtask |
| RD-M5-025 | delete task |
| RD-M5-026 | drag before/after/child visual |
| RD-M5-027 | drag auto scroll |
| RD-M5-028 | reparent protection |
| RD-M5-029 | indent/outdent |
| RD-M5-030 | same-document reorder |

## 11.5 Inspector

| ID | 任务 |
|---|---|
| RD-M5-031 | title editor |
| RD-M5-032 | status |
| RD-M5-033 | percent done |
| RD-M5-034 | priority |
| RD-M5-035 | start date |
| RD-M5-036 | due date |
| RD-M5-037 | tags |
| RD-M5-038 | categories |
| RD-M5-039 | more-properties shell |
| RD-M5-040 | unsupported property read-only surface |

## 11.6 Keyboard / Undo

| ID | 任务 |
|---|---|
| RD-M5-041 | Arrow navigation |
| RD-M5-042 | Enter edit |
| RD-M5-043 | Space completion |
| RD-M5-044 | Tab indent |
| RD-M5-045 | Shift+Tab outdent |
| RD-M5-046 | Delete |
| RD-M5-047 | Ctrl+Z |
| RD-M5-048 | Ctrl+Y |
| RD-M5-049 | Ctrl+S |
| RD-M5-050 | Ctrl+N / new task |
| RD-M5-051 | Undo UI state |
| RD-M5-052 | command coalescing front-end integration |

## 11.7 多选与基础筛选

| ID | 任务 |
|---|---|
| RD-M5-053 | Ctrl/Shift multi-select |
| RD-M5-054 | bulk complete |
| RD-M5-055 | bulk priority |
| RD-M5-056 | bulk date |
| RD-M5-057 | bulk tag |
| RD-M5-058 | filter state model |
| RD-M5-059 | title filter |
| RD-M5-060 | tag filter |
| RD-M5-061 | due filter |
| RD-M5-062 | filtered view uses TaskKey only |
| RD-M5-063 | disable/validate invalid filtered drag |
| RD-M5-064 | clear filters |

## 11.8 测试与回归任务

| ID | 场景 | 通过标准 |
|---|---|---|
| QA-M5-001 | 新建→保存→重开 | 任务一致 |
| QA-M5-002 | 深层子任务创建 | 层级一致 |
| QA-M5-003 | 删除父任务 Undo | 完整子树恢复 |
| QA-M5-004 | 同文件 drag reorder | XML 顺序一致 |
| QA-M5-005 | 拖到自己 descendant | 被拒绝 |
| QA-M5-006 | Filter 后编辑标题 | 修改真实 Task，不是副本 |
| QA-M5-007 | Filter 后修改 Due | 保存正确 |
| QA-M5-008 | Filter 后 tree operation | 不发生任务消失 |
| QA-M5-009 | Multi-select bulk edit Undo | 一致回滚 |
| QA-M5-010 | 键盘全流程 | 可不使用鼠标完成基础编辑 |
| QA-M5-011 | Autosave UI | 状态与 Core 一致 |
| QA-M5-012 | Inspector stale revision | 正确处理冲突 |
| QA-M5-013 | 关闭 dirty session | flush/错误提示正确 |
| QA-M5-014 | current XML regression | 旧文件仍可编辑 |
| QA-M5-015 | M2/M3/M4 全回归 | 无数据层退化 |
| QA-M5-016 | 典型工作区手工 dogfood | 连续操作不出现阻断错误 |

## 11.9 Exit Gate

M5 结束后必须达到：

> 新架构已经可以取代旧 Web 版本完成所有核心日常任务操作。

不得存在：

- 筛选副本编辑；
- Nested delete 复活；
- 保存编码错误；
- 无法 Undo 的基本编辑；
- 必须终端启动。

这是第一个可供内部长期 Dogfood 的 Alpha。

---

# 12. M6 — Task Relations Beta

## 12.1 阶段目标

完成任务作为“项目工作项”需要的核心关系能力：

- Participants；
- Dependencies；
- Progress Links；
- Attachments。

对应：

```text
E08
E09
```

## 12.2 Milestone 成果

> **M6 — Task Relations Beta**

## 12.3 Participants

| ID | 任务 |
|---|---|
| RD-M6-001 | Participant domain |
| RD-M6-002 | XML native mapping |
| RD-M6-003 | participant suggestion index |
| RD-M6-004 | Inspector chips |
| RD-M6-005 | add/remove participant |
| RD-M6-006 | bulk participant edit |
| RD-M6-007 | participant filter |
| RD-M6-008 | participant smart grouping |

## 12.4 Dependencies

| ID | 任务 |
|---|---|
| RD-M6-009 | TaskRef domain |
| RD-M6-010 | Local dependency parser |
| RD-M6-011 | External dependency parser |
| RD-M6-012 | Unresolved raw dependency preservation |
| RD-M6-013 | add dependency |
| RD-M6-014 | remove dependency |
| RD-M6-015 | reverse dependency index |
| RD-M6-016 | cycle detection |
| RD-M6-017 | dependency task picker |
| RD-M6-018 | incoming/outgoing UI |
| RD-M6-019 | blocked state indicator |
| RD-M6-020 | jump to dependency |
| RD-M6-021 | missing dependency handling |
| RD-M6-022 | legacy cycle warning |

## 12.5 Progress Links

| ID | 任务 |
|---|---|
| RD-M6-023 | ProgressLink domain |
| RD-M6-024 | storage mapping via validated custom field/extension |
| RD-M6-025 | URL validation |
| RD-M6-026 | provider detection |
| RD-M6-027 | Inspector UI |
| RD-M6-028 | open default browser |
| RD-M6-029 | multiple progress links policy |
| RD-M6-030 | safe unknown extension preservation |

## 12.6 Attachments

| ID | 任务 |
|---|---|
| RD-M6-031 | AttachmentKind model |
| RD-M6-032 | Managed attachment directory policy |
| RD-M6-033 | managed file import |
| RD-M6-034 | linked local file |
| RD-M6-035 | URL attachment |
| RD-M6-036 | safe filename |
| RD-M6-037 | asset UUID |
| RD-M6-038 | relative path writer |
| RD-M6-039 | attachment XML mapping |
| RD-M6-040 | attachment index |
| RD-M6-041 | attachment list UI |
| RD-M6-042 | drag file into task |
| RD-M6-043 | open attachment via shell |
| RD-M6-044 | reveal in Explorer |
| RD-M6-045 | missing file UI |
| RD-M6-046 | remove link vs delete managed |
| RD-M6-047 | BLAKE3 metadata |
| RD-M6-048 | orphan candidate tracking |

## 12.7 测试与回归任务

| ID | 场景 |
|---|---|
| QA-M6-001 | 多参与者 XML round-trip |
| QA-M6-002 | 删除/新增参与者不影响未知字段 |
| QA-M6-003 | Dependency local round-trip |
| QA-M6-004 | External dependency round-trip |
| QA-M6-005 | Unresolved dependency preservation |
| QA-M6-006 | 新建 cycle 被拒绝 |
| QA-M6-007 | legacy cycle 不被自动删除 |
| QA-M6-008 | Reverse dependency index rebuild |
| QA-M6-009 | Progress Link 删除 index.db 后仍可恢复 |
| QA-M6-010 | javascript:/危险 scheme 被拒绝 |
| QA-M6-011 | Managed attachment copy 完整性 |
| QA-M6-012 | Linked attachment 不复制原文件 |
| QA-M6-013 | 同名 attachment 不覆盖 |
| QA-M6-014 | missing attachment 不导致 task 失败 |
| QA-M6-015 | 删除 attachment 后 Undo |
| QA-M6-016 | Workspace 搬迁后 relative attachment 可用 |
| QA-M6-017 | UNC linked attachment |
| QA-M6-018 | M0–M5 全回归 |

## 12.8 Exit Gate

- Participants/Dependency/Progress/Attachment 都真实持久化进 XML/兼容扩展，不依赖 SQLite 唯一存储；
- 删除 index.db 后全部业务字段恢复；
- Dependency cycle policy 生效；
- Attachment 的“复制”和“链接”语义明确；
- 删除/撤销不会提前物理清理仍可能被引用的资产。

---

# 13. M7 — Rich Content Beta

## 13.1 阶段目标

建立安全、有限、可保真的富文本描述系统，而不是做小型 Word。

对应：

```text
E10
```

## 13.2 Milestone 成果

> **M7 — Rich Content Beta**

## 13.3 开发任务

| ID | 任务 |
|---|---|
| RD-M7-001 | 集成 Tiptap OSS minimal |
| RD-M7-002 | editor/preview separation |
| RD-M7-003 | toolbar: bold/italic/underline/strike |
| RD-M7-004 | heading |
| RD-M7-005 | limited font size |
| RD-M7-006 | text color |
| RD-M7-007 | bullet/ordered list |
| RD-M7-008 | blockquote |
| RD-M7-009 | link |
| RD-M7-010 | inline code/code block |
| RD-M7-011 | HTML whitelist sanitizer |
| RD-M7-012 | paste sanitizer |
| RD-M7-013 | dangerous URL sanitation |
| RD-M7-014 | Plain Text read/edit mode |
| RD-M7-015 | explicit Plain→Rich conversion |
| RD-M7-016 | HTML Comments adapter |
| RD-M7-017 | RTF read-only surface |
| RD-M7-018 | Unknown Comments read-only surface |
| RD-M7-019 | clipboard image capture |
| RD-M7-020 | image Asset Service |
| RD-M7-021 | image relative URL |
| RD-M7-022 | drag image |
| RD-M7-023 | image open/reveal |
| RD-M7-024 | image orphan tracking |
| RD-M7-025 | image GC command |
| RD-M7-026 | Rich Text Undo integration |
| RD-M7-027 | Rich Text autosave debounce |
| RD-M7-028 | description plain-text extraction for search |

## 13.4 测试与回归任务

| ID | 场景 |
|---|---|
| QA-M7-001 | Plain Text 打开保存无编辑，类型不改变 |
| QA-M7-002 | Plain Text 编辑，仍为 Plain Text |
| QA-M7-003 | Explicit Plain→Rich 转换需用户操作 |
| QA-M7-004 | HTML Rich Text round-trip |
| QA-M7-005 | RTF 无关字段编辑后 payload 不变 |
| QA-M7-006 | Unknown Comments 无关编辑后 payload 不变 |
| QA-M7-007 | script tag paste 被清洗 |
| QA-M7-008 | onerror/event handler 被清洗 |
| QA-M7-009 | javascript: URL 被清洗 |
| QA-M7-010 | 图片粘贴保存为外部 asset |
| QA-M7-011 | XML 不出现默认 Base64 大图 |
| QA-M7-012 | Workspace 搬迁后图片可显示 |
| QA-M7-013 | Undo 图片插入 |
| QA-M7-014 | Undo 删除图片不会立即丢文件 |
| QA-M7-015 | Orphan GC 不删 Recovery/Undo 引用资产 |
| QA-M7-016 | 大文本输入不触发每键全 XML snapshot |
| QA-M7-017 | M0–M6 全回归 |

## 13.5 Exit Gate

必须保证：

- 富文本不会自动吞掉旧 comments；
- Unsupported type 默认只读；
- 图片外置；
- sanitizer 有自动测试；
- Rich Text 不绕过 Rust Core 数据写路径；
- 删除 SQLite 仍能从 XML + Assets 恢复。

---

# 14. M8 — Multi-Document Beta

## 14.1 阶段目标

完成跨文件任务迁移、现代文件管理和 Trash。

对应：

```text
E11
E13
```

## 14.2 Milestone 成果

> **M8 — Multi-Document Beta**

核心承诺：

> 跨文件 Move 在任意崩溃点都不得让任务同时从 Source 和 Target 消失。

## 14.3 Transfer Core

| ID | 任务 |
|---|---|
| RD-M8-001 | TransferManifest |
| RD-M8-002 | source subtree snapshot |
| RD-M8-003 | target TaskId allocation |
| RD-M8-004 | ID map |
| RD-M8-005 | subtree clone |
| RD-M8-006 | internal dependency remap |
| RD-M8-007 | external dependency policy |
| RD-M8-008 | progress link preservation |
| RD-M8-009 | managed attachment manifest |
| RD-M8-010 | linked attachment preservation |
| RD-M8-011 | target asset staging |
| RD-M8-012 | target XML staging |
| RD-M8-013 | target validate |
| RD-M8-014 | target atomic commit |
| RD-M8-015 | source deletion stage |
| RD-M8-016 | source atomic commit |
| RD-M8-017 | transaction journal phases |
| RD-M8-018 | incomplete transfer recovery |
| RD-M8-019 | duplicate-after-crash detection |
| RD-M8-020 | transaction-level Undo |
| RD-M8-021 | copy operation |
| RD-M8-022 | move operation |
| RD-M8-023 | transfer error UI DTO |

## 14.4 Transfer UX

| ID | 任务 |
|---|---|
| RD-M8-024 | context menu Copy to |
| RD-M8-025 | context menu Move to |
| RD-M8-026 | target document picker |
| RD-M8-027 | target parent picker |
| RD-M8-028 | drag task onto document |
| RD-M8-029 | transfer progress |
| RD-M8-030 | conflict result dialog |
| RD-M8-031 | duplicate warning/recovery surface |

## 14.5 File Library

| ID | 任务 |
|---|---|
| RD-M8-032 | new document directly in workspace |
| RD-M8-033 | rename managed document |
| RD-M8-034 | move managed document |
| RD-M8-035 | duplicate document |
| RD-M8-036 | import managed document |
| RD-M8-037 | link external document |
| RD-M8-038 | remove reference |
| RD-M8-039 | managed Trash directory |
| RD-M8-040 | trash manifest |
| RD-M8-041 | restore from trash |
| RD-M8-042 | permanently empty trash |
| RD-M8-043 | reveal document in Explorer |
| RD-M8-044 | document operation Undo policy |
| RD-M8-045 | task reference repair after document rename/move |

## 14.6 故障注入回归矩阵

| ID | 强制故障点 | 最低可接受结果 |
|---|---|---|
| QA-M8-001 | transfer start 前 | Source 完整 |
| QA-M8-002 | target asset copy 中 | Source 完整，staging 可清理 |
| QA-M8-003 | target temp XML 写入中 | Source 完整 |
| QA-M8-004 | target validate 后、commit 前 | Source 完整 |
| QA-M8-005 | target commit 后 kill | Source + Target 可能同时存在，但不能都没有 |
| QA-M8-006 | source delete stage 前 kill | Target 已存在，Source 仍存在 |
| QA-M8-007 | source temp 写入中 kill | Source 原文件仍有效，Target 已存在 |
| QA-M8-008 | source commit 后 kill | Target 存在，Source 已正确删除 |
| QA-M8-009 | transaction journal 损坏 | 保守判断，不主动删除任何唯一副本 |
| QA-M8-010 | disk full target | Source 不动 |
| QA-M8-011 | target permission denied | Source 不动 |
| QA-M8-012 | source permission denied after target | Target 保留，Source 保留，报告“复制成功/移动未完成” |
| QA-M8-013 | dependency remap | 内部引用指向新 ID |
| QA-M8-014 | external dependency | 按 policy 保留/重写，不静默删除 |
| QA-M8-015 | managed attachment transfer | target asset 完整 |
| QA-M8-016 | linked attachment transfer | 不擅自复制 |
| QA-M8-017 | Trash restore | XML + asset 一起恢复 |
| QA-M8-018 | Linked document remove | 真实外部文件不删除 |
| QA-M8-019 | File rename external watcher | 索引和路径更新 |
| QA-M8-020 | M0–M7 全回归 | 无回退 |

## 14.7 Exit Gate

这是 P0 数据安全 Gate：

- QA-M8-001–012 任何一项失败都阻断 Milestone；
- Move 故障方向只能是“重复/待恢复”，不能是“丢失”；
- Transfer Journal 经 startup recovery 可解释；
- Asset 与 Dependency 同步进入事务；
- Trash 可恢复；
- Linked File 不被误删。

---

# 15. M9 — Productivity Feature Complete

## 15.1 阶段目标

在数据与核心功能稳定后增加成熟 To-do 产品的效率层。

对应：

```text
E12
```

## 15.2 Milestone 成果

> **M9 — Productivity Feature Complete**

此时功能集达到 2.0 Feature Complete。

## 15.3 Global Search

| ID | 任务 |
|---|---|
| RD-M9-001 | FTS5 table/migration |
| RD-M9-002 | title indexing |
| RD-M9-003 | description plain text indexing |
| RD-M9-004 | tag indexing |
| RD-M9-005 | participant indexing |
| RD-M9-006 | attachment name indexing |
| RD-M9-007 | progress-link label indexing |
| RD-M9-008 | search result DTO |
| RD-M9-009 | global search UI |
| RD-M9-010 | search keyboard navigation |
| RD-M9-011 | result jump |
| RD-M9-012 | Chinese substring/tokenizer fallback |
| RD-M9-013 | incremental FTS update |

## 15.4 Smart Views

| ID | 任务 |
|---|---|
| RD-M9-014 | Today |
| RD-M9-015 | Upcoming Agenda |
| RD-M9-016 | Overdue |
| RD-M9-017 | Unscheduled |
| RD-M9-018 | Completed |
| RD-M9-019 | Flagged（若兼容字段已确定） |
| RD-M9-020 | participant view |
| RD-M9-021 | tag view |

## 15.5 Saved Views

| ID | 任务 |
|---|---|
| RD-M9-022 | structured filter model |
| RD-M9-023 | AND/OR/NOT conditions |
| RD-M9-024 | date conditions |
| RD-M9-025 | participant conditions |
| RD-M9-026 | dependency conditions |
| RD-M9-027 | attachment conditions |
| RD-M9-028 | save view |
| RD-M9-029 | rename/delete view |
| RD-M9-030 | Saved View UI |

## 15.6 Command Palette / Quick Add

| ID | 任务 |
|---|---|
| RD-M9-031 | Command Registry finalize |
| RD-M9-032 | Ctrl+K palette |
| RD-M9-033 | task quick jump |
| RD-M9-034 | command fuzzy filter |
| RD-M9-035 | Q Quick Add |
| RD-M9-036 | token parser |
| RD-M9-037 | date parser: today/tomorrow/weekdays |
| RD-M9-038 | priority token |
| RD-M9-039 | participant token |
| RD-M9-040 | tag/list token |
| RD-M9-041 | ambiguous input fallback |
| RD-M9-042 | optional global shortcut |
| RD-M9-043 | global shortcut conflict handling |

## 15.7 测试与回归任务

| ID | 场景 |
|---|---|
| QA-M9-001 | Search title |
| QA-M9-002 | Search participant |
| QA-M9-003 | Search description |
| QA-M9-004 | 中文标题 search |
| QA-M9-005 | 删除 index.db 后 rebuild FTS |
| QA-M9-006 | Today 日期边界 |
| QA-M9-007 | Upcoming 跨月/跨年 |
| QA-M9-008 | Overdue completed exclusion |
| QA-M9-009 | Saved View reload |
| QA-M9-010 | Saved View 不改变 XML |
| QA-M9-011 | Quick Add 简单输入 |
| QA-M9-012 | Quick Add 错误 token 不吞标题 |
| QA-M9-013 | Command shortcut conflict |
| QA-M9-014 | Search result jump 多文档 |
| QA-M9-015 | Filter 后编辑仍指向 Canonical Task |
| QA-M9-016 | M0–M8 全回归 |

## 15.8 Exit Gate

- 2.0 功能范围全部完成；
- Smart View 是投影而非复制数据；
- Search index 可重建；
- Saved View 不创建新任务数据；
- Quick Add 不依赖网络/AI；
- 进入 Release Candidate 后原则上不再加入新的 2.0 功能。

---

# 16. M10 — Performance, Portable Hardening & GA

## 16.1 阶段目标

不再追求新功能，集中把 Feature Complete 版本变成可正式发布的 2.0。

对应：

```text
E14
E15
```

## 16.2 Milestone 成果

阶段分两步：

```text
M10-RC
→ Release Candidate

M10-GA
→ ModernToDoList 2.0
```

RC 后只允许：

- bug fix；
- performance fix；
- compatibility fix；
- security fix；
- usability blocker fix。

禁止新增 scope。

## 16.3 Benchmark Harness

| ID | 任务 |
|---|---|
| RD-M10-001 | deterministic fixture generator |
| RD-M10-002 | 1k task dataset |
| RD-M10-003 | 10k task dataset |
| RD-M10-004 | 50k task dataset |
| RD-M10-005 | 100k stress dataset |
| RD-M10-006 | deep-tree dataset |
| RD-M10-007 | rich-text-heavy dataset |
| RD-M10-008 | attachment-heavy index dataset |
| RD-M10-009 | dependency-heavy dataset |
| RD-M10-010 | benchmark result JSON/report |

## 16.4 Profiling

| ID | 任务 |
|---|---|
| RD-M10-011 | cold startup profiling |
| RD-M10-012 | workspace scan profiling |
| RD-M10-013 | XML parse profiling |
| RD-M10-014 | index rebuild profiling |
| RD-M10-015 | search profiling |
| RD-M10-016 | tree render profiling |
| RD-M10-017 | inspector edit latency profiling |
| RD-M10-018 | autosave profiling |
| RD-M10-019 | cross-file move profiling |
| RD-M10-020 | memory profiling |
| RD-M10-021 | WebView2 cache growth profiling |
| RD-M10-022 | decide virtualization based on evidence |
| RD-M10-023 | implement virtualization only if gate exceeded |

## 16.5 Portable Hardening

| ID | 任务 |
|---|---|
| RD-M10-024 | release ZIP deterministic build |
| RD-M10-025 | versioned Data migration |
| RD-M10-026 | old settings migration |
| RD-M10-027 | old index migration/rebuild fallback |
| RD-M10-028 | WebView2 runtime diagnostic |
| RD-M10-029 | offline package policy/documentation |
| RD-M10-030 | removable drive behavior |
| RD-M10-031 | Chinese path |
| RD-M10-032 | long path |
| RD-M10-033 | read-only EXE directory handling |
| RD-M10-034 | antivirus false-positive investigation process |
| RD-M10-035 | crash log export |
| RD-M10-036 | diagnostics export without user content |
| RD-M10-037 | LICENSES final |
| RD-M10-038 | version info/icon/metadata |
| RD-M10-039 | portable update manual path |
| RD-M10-040 | release notes template |

## 16.6 RC 全量回归矩阵

### A. XML Compatibility

| ID | 回归 |
|---|---|
| QA-M10-A01 | UTF-8 |
| QA-M10-A02 | UTF-8 BOM |
| QA-M10-A03 | UTF-16LE |
| QA-M10-A04 | UTF-16BE |
| QA-M10-A05 | unknown attr |
| QA-M10-A06 | unknown element |
| QA-M10-A07 | custom attribute |
| QA-M10-A08 | Plain Comments |
| QA-M10-A09 | HTML Comments |
| QA-M10-A10 | Unsupported Comments preservation |
| QA-M10-A11 | deep nested tasks |
| QA-M10-A12 | dependency fixture |
| QA-M10-A13 | filelink fixture |

### B. Save / Recovery

| ID | 回归 |
|---|---|
| QA-M10-B01 | normal atomic save |
| QA-M10-B02 | kill-before-write |
| QA-M10-B03 | kill-mid-write |
| QA-M10-B04 | kill-before-replace |
| QA-M10-B05 | disk full |
| QA-M10-B06 | permission denied |
| QA-M10-B07 | external clean reload |
| QA-M10-B08 | external dirty conflict |
| QA-M10-B09 | startup recovery |
| QA-M10-B10 | stale revision |

### C. Workspace / Database

| ID | 回归 |
|---|---|
| QA-M10-C01 | create/open workspace |
| QA-M10-C02 | managed docs |
| QA-M10-C03 | linked docs |
| QA-M10-C04 | relative relocation |
| QA-M10-C05 | delete index.db rebuild |
| QA-M10-C06 | corrupt index rebuild |
| QA-M10-C07 | missing workspace relocate |
| QA-M10-C08 | UNC basic |
| QA-M10-C09 | watcher burst |
| QA-M10-C10 | no watcher fallback |

### D. Core UX

| ID | 回归 |
|---|---|
| QA-M10-D01 | create/delete task |
| QA-M10-D02 | nested task |
| QA-M10-D03 | reorder/reparent |
| QA-M10-D04 | filter/edit canonical task |
| QA-M10-D05 | multi-select |
| QA-M10-D06 | keyboard workflow |
| QA-M10-D07 | undo/redo |
| QA-M10-D08 | autosave state |
| QA-M10-D09 | sidebar restore |
| QA-M10-D10 | inspector restore |

### E. Relations / Attachments / Rich Text

| ID | 回归 |
|---|---|
| QA-M10-E01 | participants |
| QA-M10-E02 | dependency add/remove |
| QA-M10-E03 | dependency cycle |
| QA-M10-E04 | progress link |
| QA-M10-E05 | managed attachment |
| QA-M10-E06 | linked attachment |
| QA-M10-E07 | missing attachment |
| QA-M10-E08 | rich plain text |
| QA-M10-E09 | rich HTML |
| QA-M10-E10 | unsupported comment |
| QA-M10-E11 | image paste |
| QA-M10-E12 | asset relocation |

### F. Cross-document

| ID | 回归 |
|---|---|
| QA-M10-F01 | copy subtree |
| QA-M10-F02 | move subtree |
| QA-M10-F03 | internal dependency remap |
| QA-M10-F04 | external dependency policy |
| QA-M10-F05 | managed asset move |
| QA-M10-F06 | move crash target-before-source |
| QA-M10-F07 | move disk full |
| QA-M10-F08 | move source permission denied |
| QA-M10-F09 | transaction recovery |
| QA-M10-F10 | transfer undo |

### G. Productivity

| ID | 回归 |
|---|---|
| QA-M10-G01 | global search |
| QA-M10-G02 | Chinese search |
| QA-M10-G03 | Today |
| QA-M10-G04 | Upcoming |
| QA-M10-G05 | Overdue |
| QA-M10-G06 | Saved View |
| QA-M10-G07 | Quick Add |
| QA-M10-G08 | Command Palette |
| QA-M10-G09 | shortcut |
| QA-M10-G10 | search after rebuild |

### H. Portable / Clean Machine

| ID | 回归 |
|---|---|
| QA-M10-H01 | Windows 11 clean VM |
| QA-M10-H02 | Windows 10 clean VM |
| QA-M10-H03 | no network |
| QA-M10-H04 | no admin |
| QA-M10-H05 | no Node/Rust |
| QA-M10-H06 | no terminal |
| QA-M10-H07 | move app directory |
| QA-M10-H08 | move relative workspace together |
| QA-M10-H09 | removable drive |
| QA-M10-H10 | Chinese/space path |
| QA-M10-H11 | WebView2 UDF in Data |
| QA-M10-H12 | close app leaves no ModernToDoList service |
| QA-M10-H13 | delete cache |
| QA-M10-H14 | delete index |
| QA-M10-H15 | delete whole app directory = uninstall |

## 16.7 RC Blocker 分类

以下任何问题都自动判定 RC Blocker：

### Blocker-Data

- 数据丢失。
- 未知 XML 字段消失。
- Encoding 被错误转换。
- Comments 类型被静默改变。
- Move 造成 Source/Target 都没有任务。
- Attachment 唯一副本被误删。

### Blocker-Persistence

- Save truncate 原文件。
- Recovery 无法识别已知中断状态。
- 外部修改被静默覆盖。

### Blocker-Portable

- 正常启动必须管理员权限。
- 必须 Node/Terminal。
- 关键业务数据只存在 AppData。
- 关闭后留下必须存在的后台服务。

### Blocker-Security

- Rich Text 可执行 script。
- URL 可通过 `javascript:` 执行。
- Managed path 可以逃出 Asset Root。

### Blocker-Core UX

- 无法打开/保存主流 TDL/XML。
- 基础任务操作导致 crash。
- Index 损坏使业务 XML 无法打开。

## 16.8 GA Exit Gate

GA 必须满足：

- 所有 Blocker = 0；
- 所有 P0/P1 已知数据安全问题 = 0；
- M0–M9 自动测试全通过；
- RC 全量矩阵通过；
- 至少完成一次“删除 index.db → 重建 → 全功能验证”；
- 至少完成一次“跨文件 Move 强杀恢复”；
- 至少完成一次 Windows 10 和 Windows 11 clean VM；
- 至少完成一次 no-network Portable 测试；
- Release ZIP hash 和版本固定；
- Migration test 通过；
- Release Notes 完成。

---

# 17. Milestone 回归策略

每个 Milestone 不仅测自己的功能，还必须执行“累积回归”。

| Milestone | 必须回归 |
|---|---|
| M0 | 本阶段 |
| M1 | M1 Portable |
| M2 | M0 + M2 XML |
| M3 | M0–M3 |
| M4 | M0–M4 |
| M5 | M0–M5 |
| M6 | M0–M6 |
| M7 | M0–M7 |
| M8 | M0–M8，重点 Fault Injection |
| M9 | M0–M9 |
| M10 | 全量 |

P0 自动化回归永远不得因为“慢”而取消。

如果耗时增长：

- 分 PR Gate；
- nightly；
- RC full suite；

但不能删除测试。

---

# 18. PR 级测试策略

## 普通 UI PR

要求：

```text
Q0
相关 Q1
相关 Q5 component/e2e
```

## XML PR

要求：

```text
Q0
Q1
Q2
相关 Q3 全量
```

## Save / Transfer PR

要求：

```text
Q0
Q1
Q2
Q3
相关 Q4 Fault Injection
```

## SQLite / Workspace PR

要求：

```text
Q0
Q1
Q2
DB rebuild test
XML source-of-truth regression
```

## Release PR

要求：

```text
Q0-Q7
```

---

# 19. 测试环境矩阵

最低测试环境：

```text
Windows 11 x64
Windows 10 x64
```

存储环境：

```text
NTFS local SSD
removable drive
UNC/NAS basic scenario
read-only folder
long path
Chinese path
space-containing path
```

网络：

```text
online
offline
```

运行环境：

```text
normal user
no administrator
clean machine
```

文件：

```text
UTF-8
UTF-16
large XML
deep tree
legacy comments
unknown extensions
```

---

# 20. 性能回归规则

性能优化不得以数据安全为代价。

每次性能优化 PR 必须：

1. 指明真实 benchmark。
2. 给出 before/after。
3. 保证 Golden XML 不变。
4. 保证 Fault Injection 不退化。
5. 禁止通过减少安全 flush/validation 偷换性能。

如果需要在性能和数据安全中选择：

> 默认优先数据安全。

---

# 21. Backlog 优先级规则

## P0

- 数据损坏；
- XML 兼容；
- Save/Recovery；
- Portable 启动；
- Move 数据安全；
- Security。

## P1

- 核心任务操作；
- Workspace；
- Participants；
- Dependencies；
- Attachments；
- Rich Text；
- Search；
- Smart Views。

## P2

- UX polish；
- Calendar；
- Kanban；
- Review；
- Templates。

## P3

- 高级自动化；
- 可选系统集成。

任何 P2/P3 不得阻塞 2.0，只要没有影响 P0/P1 完整性。

---

# 22. Issue 拆分规范

一个 Issue 应尽量只有一个可验证结果。

错误示例：

```text
实现附件系统
```

推荐拆成：

```text
Attachment Domain Model
Managed Attachment Import
Linked File Attachment
Attachment XML Mapping
Attachment Index
Attachment UI
Attachment Open
Attachment Recovery
Attachment Round-trip Tests
```

每个 Issue 必须包含：

```text
Context
Scope
Out of Scope
Architecture Impact
Data Safety Impact
Dependencies
Acceptance Criteria
Tests
Recovery Behavior
```

---

# 23. Milestone 完成报告模板

关闭每个 Milestone 前必须提交：

```markdown
# Milestone Mx Completion Report

## Delivered
...

## Deferred
...

## Architecture Deviations
...

## Test Results
- Unit:
- Integration:
- Golden:
- Fault:
- E2E:
- Portable:

## Known Issues
...

## Data Safety Review
...

## Regression Result
...

## Exit Gate
PASS / FAIL
```

如果 Architecture Deviations 非空：

必须说明：

- 为什么偏离原设计；
- 新依据是什么；
- 是否更新 ADR；
- 是否需要更新架构书。

---

# 24. Regression Baseline Freeze

每个已修复 P0/P1 Bug 都必须转化为永久回归测试。

例如当前已知：

```text
BUG-Filter-Clone
BUG-Nested-Task-Delete
BUG-Comment-Type-Overwrite
BUG-Encoding-Mismatch
```

修复后：

```text
tests/regressions/
```

加入固定测试。

规则：

> 修过一次的数据安全 Bug，不允许只靠“记住不要再犯”。

---

# 25. 发布分支策略

建议：

```text
main
    稳定主线

next-desktop
    2.0 集成分支（开发早期）

feature/*
fix/*
```

达到 M5 Alpha 后：

考虑将新桌面架构成为 `main`，旧 Web 版进入 maintenance tag。

RC：

```text
release/2.0
```

RC 分支只允许：

- blocker fix；
- regression fix；
- release metadata。

禁止再合新功能。

---

# 26. Release Artifact

最终：

```text
ModernToDoList-2.0.0-Portable-win-x64.zip
```

包含：

```text
ModernToDoList.exe
LICENSES/
README.md
```

运行后创建：

```text
Data/
```

如果未来提供 Fixed WebView2：

```text
ModernToDoList-2.0.0-Portable-Offline-win-x64.zip
```

作为单独构建，不增加默认 Portable 体积。

---

# 27. 2.0 GA 最终能力清单

## Desktop / Portable

- Windows Portable EXE。
- 无安装。
- 无后台服务。
- 无终端。
- Offline。
- Single Instance。
- Portable Data。

## Workspace

- 多 Workspace。
- Managed/Linked Document。
- 自动扫描。
- 索引重建。
- 路径迁移。
- 外部修改检测。

## Task

- 无限层级任务。
- Add/Delete/Move。
- Start/Due。
- Status/Progress。
- Priority。
- Tags/Categories。
- Multi-select。
- Undo/Redo。

## Relations

- Participants。
- Dependencies。
- Progress Links。

## Content

- Plain/Rich Description。
- 图片。
- Managed Attachment。
- Linked File。
- URL Attachment。

## Multi-document

- Copy。
- Move。
- ID remap。
- Dependency remap。
- Attachment transaction。
- Recovery。

## Productivity

- Global Search。
- Today。
- Upcoming。
- Overdue。
- Unscheduled。
- Saved Views。
- Quick Add。
- Command Palette。
- Keyboard-first。

## Data Safety

- Semantic Lossless XML。
- UTF-8/UTF-16。
- Unknown fields。
- Comments type protection。
- Atomic Save。
- Recovery Journal。
- Fingerprint。
- External Conflict。
- Database Rebuild。
- Trash。

---

# 28. 2.0 明确不进入主 Roadmap 的功能

以下功能不进入 2.0 Critical Path：

- 完整 Calendar。
- Kanban。
- Review。
- Templates。
- Pomodoro。
- Habit。
- AI。
- Cloud Sync。
- Team Collaboration。
- Plugin Marketplace。
- Web Version。
- Mobile Version。
- Electron。
- Tauri Auto Updater。
- 完整 RTF 编辑。
- Git-like XML merge。

它们只能在 2.0 GA 后根据真实使用反馈进入 2.x/3.0 评估。

---

# 29. Roadmap 成功判定

这个 Roadmap 最终不是以：

```text
完成多少 Issue
```

来判定成功。

而以以下事实判定：

1. 用户能够把旧 TDL/XML 文件直接打开并安全编辑。
2. 用户不需要安装、不需要终端、不需要后台服务。
3. 用户能够使用 Workspace 管理大量本地任务文件。
4. SQLite 删除后业务数据不丢。
5. 未知 XML 数据不会因为 ModernToDoList 不理解而消失。
6. 富文本不会破坏旧 Comments。
7. 跨文件 Move 任意中断不丢任务。
8. 外部软件修改文件不会被静默覆盖。
9. 附件和图片能够随 Managed Workspace 正确迁移。
10. 搜索和 Smart Views 不产生第二份任务数据。
11. 10k–50k 级真实任务工作区仍具备可接受的交互效率。
12. 2.0 Release ZIP 在 Clean Windows 机器上解压即用。

满足以上条件，才意味着 ModernToDoList 从“本地 XML 网页编辑器”真正升级为一个成熟的 Windows Portable 本地任务管理产品。

---

# 30. 下一步执行顺序

正式进入开发时，不需要再次重做架构讨论。

第一批实际任务应直接建立为：

```text
M0
├── RD-M0-001 Baseline Freeze
├── RD-M0-002 Current Fixture Capture
├── RD-M0-003 Real TDL Corpus
├── RD-M0-004 Encoding Corpus
├── RD-M0-005~020 Schema Audit
└── QA-M0-* Compatibility Validation

并行：
M1
├── RD-M1-001 Vue/TS/Vite
├── RD-M1-002 Tauri
├── RD-M1-007 Portable Root
└── RD-M1-009 WebView2 UDF
```

M0 结束后立即进入：

```text
M2 Lossless XML Core
```

而不是先继续增加 UI 功能。

M2/M3 通过后，整个项目才真正具备安全扩展能力。

---

# 31. Roadmap 与架构书的约束关系

本文是《ModernToDoList 2.0 — 产品与详细技术架构设计书》的执行层。

优先级：

```text
Architecture Invariants
        ↓
Roadmap Milestone
        ↓
Epic
        ↓
Issue
        ↓
PR
```

Issue 或 PR 如果与架构 Invariant 冲突：

不得以“实现更快”为理由直接合并。

必须：

1. 提出 ADR；
2. 说明新证据；
3. 更新架构书；
4. 更新 Roadmap；
5. 再实施。

这可以防止项目在数十个 Issue 之后逐渐偏离最初的“数据保真、本地化、Portable、轻量”目标。
