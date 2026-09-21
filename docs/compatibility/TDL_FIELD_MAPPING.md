# TDL 字段映射表

> 对应任务：RD-M0-005 ~ RD-M0-017  
> 基线样本：`tests/fixtures/xml/real-world/london-underground.xml`（原始 `lists/test.xml`）  
> 当前版本代码：`script.js` L773-826（parseTaskNode）、L829-895（updateNodeFromTask）  
> 日期：2026-09-13

---

## 1. TODOLIST 根元素属性

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| PROJECTNAME | attribute | `""` | 否 | **Tier B** | 项目名称，保留但不编辑 |
| EARLIESTDUEDATE | attribute | `"0.00000000"` | 否 | **Tier C** | 最早截止日期（OLE 日期格式），只读保留 |
| LASTMOD | attribute | `"46100.63025463"` | 否 | **Tier A** | 最后修改时间（OLE 自动化日期浮点数），2.0 必须正确更新 |
| LASTMODSTRING | attribute | `"2026/3/19 15:07"` | 否 | **Tier A** | 最后修改时间字符串表示，2.0 必须同步更新 |
| FILENAME | attribute | `"London Underground.xml"` | 否 | **Tier B** | 原始文件名，保留但不编辑 |
| NEXTUNIQUEID | attribute | `"10"` | 否 | **Tier A** | 下一个唯一 ID，2.0 必须正确维护用于 ID 分配 |
| FILEVERSION | attribute | `"43"` | 否 | **Tier B** | 文件格式版本号，保留但不编辑 |
| APPVER | attribute | `"9.0.14.0"` | 否 | **Tier B** | 创建/最后保存的应用版本，保留但不编辑 |
| FILEFORMAT | attribute | `"12"` | 否 | **Tier B** | 文件格式版本，保留但不编辑 |

---

## 2. TASK 属性

### 2.1 核心标识字段

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| ID | attribute | `"1"` | 是（读） | **Tier A** | 任务唯一标识符，整数。当前只读，2.0 需完整管理 |
| TITLE | attribute | `"South Kensigton"` | 是（读写） | **Tier A** | 任务标题，主要编辑字段 |
| REFID | attribute | `"0"` | 否 | **Tier B** | 引用 ID，用于跨文件引用。保留但不编辑 |

### 2.2 状态与进度字段

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| PRIORITY | attribute | `"5"` | 是（读写） | **Tier A** | 优先级 0-10，主要编辑字段 |
| RISK | attribute | `"0"` | 否 | **Tier A** | 风险等级 0-10，2.0 第一版应支持编辑 |
| PERCENTDONE | attribute | `"0"` | 是（读写） | **Tier A** | 完成百分比 0-100，主要编辑字段 |
| SUBTASKDONE | attribute | `"0/1"` | 否 | **Tier C** | 子任务完成统计（仅 task ID=5），只读保留。格式：`"完成数/总数"` |

### 2.3 日期时间字段

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| STARTDATE | attribute | `"45259.00000000"` | 否 | **Tier A** | 开始日期（OLE 自动化日期），2.0 必须读写 |
| STARTDATESTRING | attribute | `"2023/11/29"` | 否 | **Tier A** | 开始日期字符串，2.0 必须同步更新 |
| DUEDATE | attribute | 不存在 | 否（代码读） | **Tier A** | 截止日期（OLE 自动化日期）。test.xml 无此字段，但 AbstractSpoon 支持 |
| DUEDATESTRING | attribute | 不存在 | 否（代码读写） | **Tier A** | 截止日期字符串。test.xml 无此字段，但 AbstractSpoon 支持 |
| CREATIONDATE | attribute | `"45259.82717593"` | 否 | **Tier B** | 创建日期（OLE 自动化日期），保留但不编辑 |
| CREATIONDATESTRING | attribute | `"2023/11/29 19:51"` | 否（代码读 CREATEDATESTRING） | **Tier B** | 创建日期字符串，保留但不编辑。**注意**：当前代码错误使用 `CREATEDATESTRING` |
| COMPLETIONDATE | attribute | 不存在 | 否 | **TBD** | 完成日期。test.xml 无此字段，需确认 AbstractSpoon 实际格式 |
| COMPLETIONDATESTRING | attribute | 不存在 | 否 | **TBD** | 完成日期字符串。test.xml 无此字段，需确认 AbstractSpoon 实际格式 |
| LASTMOD | attribute | `"45259.84489583"` | 否 | **Tier A** | 最后修改时间（OLE 自动化日期），2.0 必须正确更新 |
| LASTMODSTRING | attribute | `"2023/11/29 20:16"` | 否 | **Tier A** | 最后修改时间字符串，2.0 必须同步更新 |

**OLE 自动化日期说明**：
- 格式：浮点数，整数部分表示自 1899-12-30 以来的天数
- 小数部分表示一天中的时间（0.0 = 00:00, 0.5 = 12:00）
- 示例：`45259.82717593` = 2023-11-29 19:51

### 2.4 人员与参与者字段

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| CREATEDBY | attribute | `"Daniel G"` | 否 | **Tier B** | 创建者，保留但不编辑 |
| LASTMODBY | attribute | `"Daniel G"` | 否 | **Tier A** | 最后修改者，2.0 必须正确更新 |
| ALLOCATEDTO | attribute | 不存在 | 否 | **TBD** | 分配给。test.xml 无此字段，需确认 AbstractSpoon 实际格式（可能是子元素） |
| ALLOCATEDBY | attribute | 不存在 | 否 | **TBD** | 分配者。test.xml 无此字段，需确认 AbstractSpoon 实际格式 |

### 2.5 时间与工时字段

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| TIMEESTIMATE | attribute | `"0.00000000"` | 否 | **Tier C** | 时间估算（仅 task ID=9），只读保留 |
| TIMEESTUNITS | attribute | `"D"` | 否 | **Tier C** | 时间估算单位（D=天, H=小时, M=分钟），只读保留 |
| TIMESPENT | attribute | `"0.00000000"` | 否 | **Tier C** | 实际花费时间（仅 task ID=9），只读保留 |
| TIMESPENTUNITS | attribute | `"D"` | 否 | **Tier C** | 实际花费时间单位，只读保留 |

### 2.6 显示与样式字段

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| TEXTCOLOR | attribute | `"0"` | 否 | **Tier C** | 文本颜色（RGB 整数），只读保留 |
| TEXTWEBCOLOR | attribute | `"#000000"` | 否 | **Tier C** | 文本颜色（Web 格式），只读保留 |
| PRIORITYCOLOR | attribute | `"15288168"` | 否 | **Tier C** | 优先级颜色（RGB 整数），只读保留 |
| PRIORITYWEBCOLOR | attribute | `"#E9"` | 否 | **Tier C** | 优先级颜色（Web 格式），只读保留。**注意**：test.xml 中值截断 |
| POS | attribute | `"0"` | 否 | **Tier A** | 位置索引（整数），2.0 必须维护以正确排序 |
| POSSTRING | attribute | `"1"` | 否 | **Tier B** | 位置字符串（如 `"1"`, `"1.1"`, `"1.1.1"`），保留但不编辑 |

### 2.7 注释相关字段

| 字段名 | 类型 | test.xml 示例值 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| COMMENTSTYPE | attribute | `"PLAIN_TEXT"` | 是（写，硬编码） | **Tier A** | 注释类型，受保护字段。2.0 禁止静默修改 |

---

## 3. TASK 子元素

### 3.1 已知元素

| 元素名 | 类型 | test.xml 示例 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| TASK | 子元素 | 嵌套任务 | 是（递归） | **Tier A** | 子任务，支持无限层级 |
| FILEREFPATH | 子元素 | `".\London Underground Photos\southken.jpg"` | 是（读写单个） | **Tier A** | 文件链接路径。test.xml 每个任务最多 1 个，但 AbstractSpoon 支持多个 |
| COMMENTS | 子元素 | 不存在 | 是（读写） | **Tier A** | 注释内容，类型由 COMMENTSTYPE 决定 |
| CATEGORY | 子元素 | 不存在 | 是（读写多个） | **Tier A** | 分类/标签，支持多个 |
| METADATA | 子元素 | `FA40B83E-E934-D494-8FB3-8EC9748FA4E8="..."` | 否 | **Tier B** | 自定义元数据，GUID 键值对。保留但不编辑 |

### 3.2 依赖相关元素（test.xml 无样本，需确认）

| 元素名 | 类型 | 预期格式 | 当前版本处理 | 2.0 处理策略 | 说明 |
|---|---|---|---|---|---|
| DEPENDENCY | 子元素 | `<DEPENDENCY><TASKID>2</TASKID><DEPENDENCYTYPE>0</DEPENDENCYTYPE></DEPENDENCY>` | 否 | **TBD** | 任务依赖。fixture 中已创建样本，但需真实 AbstractSpoon 样本验证 |
| DEPENDENCY/TASKID | 子元素 | `"2"` | 否 | **TBD** | 依赖的任务 ID |
| DEPENDENCY/DEPENDENCYTYPE | 子元素 | `"0"` | 否 | **TBD** | 依赖类型（0=完成-完成, 1=开始-开始, 等） |

---

## 4. 当前版本字段处理问题

### 4.1 字段名不一致

| 当前代码使用 | AbstractSpoon TDL 实际 | 问题 |
|---|---|---|
| `CREATEDATESTRING` | `CREATIONDATESTRING` | 拼写错误，导致无法正确读取创建时间 |

### 4.2 硬编码覆盖

| 字段 | 当前行为 | 问题 |
|---|---|---|
| `COMMENTSTYPE` | 写入 COMMENTS 时硬编码为 `'PLAIN_TEXT'` | 静默改变注释类型（R-003） |

### 4.3 新任务字段缺失

当前 `createNewTaskObject` 创建的新任务只包含：
- `ID`（使用 Date.now().toString(36) + 随机字符串，非整数）
- `TITLE`
- `CREATEDATESTRING`（拼写错误）

缺少所有标准属性：
- `REFID`, `COMMENTSTYPE`, `CREATEDBY`, `PRIORITY`, `RISK`, `PERCENTDONE`
- `STARTDATE`, `STARTDATESTRING`, `CREATIONDATE`, `CREATIONDATESTRING`
- `LASTMOD`, `LASTMODSTRING`, `LASTMODBY`
- `POS`, `POSSTRING`, `TEXTCOLOR`, `TEXTWEBCOLOR`, `PRIORITYCOLOR`, `PRIORITYWEBCOLOR`

### 4.4 ID 格式问题

当前代码生成的 ID 格式：`Date.now().toString(36) + Math.random().toString(36).substr(2, 5)`

示例：`"m5x8k2p9q"`（base36 字符串）

AbstractSpoon TDL 期望：纯整数（如 `"1"`, `"2"`, `"10"`）

**影响**：当前版本创建的任务无法被 AbstractSpoon ToDoList 正确识别。

---

## 5. 2.0 处理策略分级

### Tier A（必须读写）

2.0 必须完整支持读写的字段，包括正确更新和维护：

- `ID`, `TITLE`, `PRIORITY`, `RISK`, `PERCENTDONE`
- `STARTDATE`, `STARTDATESTRING`, `DUEDATE`, `DUEDATESTRING`
- `LASTMOD`, `LASTMODSTRING`, `LASTMODBY`
- `POS`, `COMMENTSTYPE`
- 子元素：`TASK`, `FILEREFPATH`, `COMMENTS`, `CATEGORY`

### Tier B（应保留但暂不编辑）

2.0 应保留这些字段不被删除或修改，但第一版不需要提供编辑界面：

- TODOLIST 属性：`PROJECTNAME`, `FILENAME`, `FILEVERSION`, `APPVER`, `FILEFORMAT`
- TASK 属性：`REFID`, `CREATIONDATE`, `CREATIONDATESTRING`, `CREATEDBY`
- 子元素：`METADATA`

### Tier C（只读保留）

2.0 只读取和保留，完全不修改：

- TODOLIST 属性：`EARLIESTDUEDATE`
- TASK 属性：`SUBTASKDONE`, `TIMEESTIMATE`, `TIMEESTUNITS`, `TIMESPENT`, `TIMESPENTUNITS`
- TASK 属性：`TEXTCOLOR`, `TEXTWEBCOLOR`, `PRIORITYCOLOR`, `PRIORITYWEBCOLOR`, `POSSTRING`

### TBD（待确认）

需要更多真实样本或 AbstractSpoon 文档确认的字段：

- `COMPLETIONDATE`, `COMPLETIONDATESTRING`
- `ALLOCATEDTO`, `ALLOCATEDBY`
- `DEPENDENCY` 元素结构

---

## 6. 字段来源验证

所有字段均来自：
1. `tests/fixtures/xml/real-world/london-underground.xml`（真实 AbstractSpoon 输出）
2. `script.js` 当前代码分析
3. AbstractSpoon ToDoList 公开文档（需进一步验证 TBD 字段）

**不允许推测**：任何未在真实样本中出现的字段格式均标记为 TBD。
