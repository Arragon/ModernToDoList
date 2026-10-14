# M0 兼容性基线冻结

> 对应任务：RD-M0-001  
> 基线 commit：`fb6ee9c` — feat: 添加任务创建时间字段、Toast通知系统和本地开发服务器  
> 冻结日期：2026-09-13  
> 基线文件：`script.js`（L773-826 `parseTaskNode`，L829-895 `updateNodeFromTask`）

---

## 1. 当前版本能力：能够读写的 XML 字段

### 1.1 读取字段（parseTaskNode）

| 字段名 | 类型 | 读取方式 | 备注 |
|---|---|---|---|
| ID | attribute | `getAttribute('ID')` | |
| TITLE | attribute | `getAttribute('TITLE')` | |
| PERCENTDONE | attribute | `getAttribute('PERCENTDONE')` | |
| PRIORITY | attribute | `getAttribute('PRIORITY')` | |
| DUEDATESTRING | attribute | `getAttribute('DUEDATESTRING')` | **BUG**：test.xml 中不存在此字段，AbstractSpoon TDL 使用 DUEDATE/DUEDATESTRING，但当前样本无此属性 |
| CREATEDATESTRING | attribute | `getAttribute('CREATEDATESTRING')` | **BUG**：test.xml 中不存在此字段；实际字段为 `CREATIONDATESTRING`（注意拼写差异） |
| FILEREFPATH | 子元素 | `child.textContent` | 只读取第一个 |
| COMMENTS | 子元素 | `child.textContent` | |
| CATEGORY | 子元素 | `child.textContent`（多个） | |
| TASK | 子元素 | 递归 `parseTaskNode` | |

### 1.2 写字段（updateNodeFromTask）

| 字段名 | 类型 | 写入方式 | 备注 |
|---|---|---|---|
| TITLE | attribute | `setAttribute('TITLE', ...)` | |
| PERCENTDONE | attribute | `setAttribute('PERCENTDONE', ...)` | |
| PRIORITY | attribute | `setAttribute('PRIORITY', ...)` | |
| DUEDATESTRING | attribute | `setAttribute('DUEDATESTRING', ...)` | 将 `-` 替换为 `/` |
| CREATEDATESTRING | attribute | `setAttribute('CREATEDATESTRING', ...)` | 字段名与 AbstractSpoon 不一致 |
| FILEREFPATH | 子元素 | 创建/更新/删除 | 只处理单个 |
| COMMENTS | 子元素 | 创建/更新/删除 | |
| COMMENTSTYPE | attribute | 硬编码 `'PLAIN_TEXT'` | **BUG**：强制覆盖原始类型（R-003） |
| CATEGORY | 子元素 | 先删除全部，再重新添加 | |

### 1.3 当前版本不处理的 test.xml 字段

以下字段在 test.xml 中存在，但当前版本**完全不读取也不写入**：

**TODOLIST 根元素属性（全部不处理）：**

| 字段 | test.xml 示例值 |
|---|---|
| PROJECTNAME | `""` |
| EARLIESTDUEDATE | `"0.00000000"` |
| LASTMOD | `"46100.63025463"` |
| LASTMODSTRING | `"2026/3/19 15:07"` |
| FILENAME | `"London Underground.xml"` |
| NEXTUNIQUEID | `"10"` |
| FILEVERSION | `"43"` |
| APPVER | `"9.0.14.0"` |
| FILEFORMAT | `"12"` |

**TASK 属性（全部不处理）：**

| 字段 | test.xml 示例值 |
|---|---|
| REFID | `"0"` |
| CREATEDBY | `"Daniel G"` |
| RISK | `"0"` |
| STARTDATE | `"45259.00000000"` |
| STARTDATESTRING | `"2023/11/29"` |
| CREATIONDATE | `"45259.82717593"` |
| CREATIONDATESTRING | `"2023/11/29 19:51"` |
| LASTMOD | `"45259.84489583"` |
| LASTMODSTRING | `"2023/11/29 20:16"` |
| LASTMODBY | `"Daniel G"` |
| POS | `"0"` |
| POSSTRING | `"1"` |
| TEXTCOLOR | `"0"` |
| TEXTWEBCOLOR | `"#000000"` |
| PRIORITYCOLOR | `"15288168"` |
| PRIORITYWEBCOLOR | `"#E9"` (截断自 `"#E9"` → 实际 `"#E9"`) |
| SUBTASKDONE | `"0/1"` |
| TIMEESTIMATE | `"0.00000000"` |
| TIMEESTUNITS | `"D"` |
| TIMESPENT | `"0.00000000"` |
| TIMESPENTUNITS | `"D"` |

**TASK 子元素（不处理）：**

| 元素 | 说明 |
|---|---|
| METADATA | 含 GUID 键的自定义属性，如 `FA40B83E-E934-D494-8FB3-8EC9748FA4E8` |

---

## 2. 已知 Bug（R-001 ~ R-006）

### R-001：筛选结果复制任务对象

**问题**：当前筛选树通过 `{ ...task, children: filteredChildren }` 对象展开生成副本。筛选状态下编辑、拖拽或重新父子化可能操作副本而不是 Canonical Task。

**影响**：筛选中编辑可能不反映到实际数据，或保存后数据不一致。

**2.0 规则**：任何视图只保存 TaskKey/TaskRef，不得复制业务实体作为可编辑对象。

### R-002：XML 子树同步可能遗留旧节点

**问题**：当前 `generateXMLStringForList` 先清除所有 TASK 节点再通过 `updateNodeFromTask` 重建。对原任务节点内部旧子 TASK 的清理和重建语义不完整，删除嵌套任务存在重新出现或保留脏节点的风险。

**影响**：删除嵌套任务后可能在保存时复活。

**2.0 规则**：XML Adapter 必须拥有唯一的结构修改入口；UI 不得直接操作 XML DOM。

### R-003：Comments 类型可能被强制改成纯文本

**问题**：`updateNodeFromTask` 在写入 COMMENTS 时硬编码 `node.setAttribute('COMMENTSTYPE', 'PLAIN_TEXT')`。如果文件原本是 HTML、RTF 或其他内容类型，会造成隐性格式损失。

**影响**：打开 HTML 格式注释的文件并保存后，注释类型被静默改为 PLAIN_TEXT。

**2.0 规则**：comment content type 是受保护字段；未知类型默认只读，不允许静默转换。

### R-004：XML 编码声明与真实写出编码可能不一致

**问题**：当前代码创建带 `encoding="utf-16"` 声明的 XML 字符串，但浏览器 File System Access API 最终以 UTF-8 或浏览器默认编码写入文本。声明与实际字节编码不匹配。

**影响**：AbstractSpoon ToDoList 打开文件时可能报告编码错误或乱码。

**2.0 规则**：编码是 DocumentMetadata 的一部分。读取时检测，保存时保持，除非用户明确执行编码转换。

### R-005：Undo 保存整份状态

**问题**：当前 Undo 通过 `serializeState()` 每次序列化整个应用状态（包含完整 XML 字符串）。随着任务、富文本、附件元数据增加，整份状态快照会产生明显内存、序列化和延迟成本。

**影响**：大文件场景下 Undo 操作卡顿，内存占用持续增长。

**2.0 规则**：使用命令式 Undo/Redo，仅记录操作而非完整状态。

### R-006：Web 环境导致本地集成受限

**问题**：
- 文件路径受沙箱限制
- 本地附件打开体验差
- 文件夹长期授权不稳定
- 无法可靠做到外部文件实时监控
- 启动依赖 Web Server
- 无原生单实例、文件拖放、全局快捷键等体验

**影响**：无法提供真正的桌面应用体验。

**2.0 规则**：正式终止"浏览器作为产品运行环境"的路线，迁移至 Tauri 2 桌面架构。

---

## 3. 当前版本行为总结

### 3.1 正确行为

- 能解析 test.xml 的基本任务结构（ID、TITLE、PERCENTDONE、PRIORITY）
- 能处理嵌套 TASK 递归解析
- 能读取和写入 FILEREFPATH（单个）
- 能读取和写入 COMMENTS 文本
- 能读取和写入 CATEGORY（多个）
- 能通过 File System Access API 打开和保存文件
- 能通过 IndexedDB 恢复工作区状态

### 3.2 数据丢失行为

- **保存时丢弃所有未处理属性**：由于 `generateXMLStringForList` 重建 TASK 树时依赖原始 XML DOM 节点（`task.node`），未修改的属性会保留在 DOM 中。但如果创建新任务，则只有 ID、TITLE、CREATEDATESTRING 三个属性。
- **COMMENTSTYPE 被覆盖**：任何有 COMMENTS 内容的任务，其 COMMENTSTYPE 会被强制设为 PLAIN_TEXT。
- **METADATA 元素保留**：由于保留原始 DOM 节点，METADATA 子元素在保存时不会丢失。
- **新任务缺少大量标准属性**：如 REFID、RISK、CREATEDBY、POS 等。

### 3.3 字段名不一致

| 当前代码使用 | AbstractSpoon TDL 实际 | 说明 |
|---|---|---|
| `CREATEDATESTRING` | `CREATIONDATESTRING` | 拼写错误 |
| `DUEDATESTRING` | `DUEDATESTRING` | test.xml 中不存在此字段，但 AbstractSpoon 确实支持 |

---

## 4. 基线快照

```text
Commit:     fb6ee9c
Message:    feat: 添加任务创建时间字段、Toast通知系统和本地开发服务器
文件:       index.html, script.js, style.css, package.json
依赖:       Vue 3 (CDN), Tailwind CSS (CDN), Font Awesome 6 (CDN)
存储:       IndexedDB + File System Access API
运行方式:   Python run.py 本地 HTTP 服务器 + 浏览器
```
