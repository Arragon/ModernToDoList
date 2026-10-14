# 依赖格式文档（DEPENDENCY_FORMAT）

> 对应任务：RD-M0-011  
> 日期：2026-09-13

---

## 1. 概述

AbstractSpoon ToDoList 支持任务之间的依赖关系。依赖信息存储在 `<DEPENDENCY>` 子元素中。

**重要**：当前 `test.xml` 样本中不包含任何依赖关系。以下格式基于 AbstractSpoon 公开文档和合理推断，**标记为 TBD**，需真实样本验证。

---

## 2. 本地依赖（同文件内）

### 2.1 XML 表示格式

```xml
<TASK ID="1" TITLE=" dependent Task" ...>
    <DEPENDENCY>
        <TASKID>2</TASKID>
        <DEPENDENCYTYPE>0</DEPENDENCYTYPE>
    </DEPENDENCY>
</TASK>
<TASK ID="2" TITLE="prerequisite Task" ...>
</TASK>
```

### 2.2 字段说明

| 字段 | 类型 | 说明 | 验证状态 |
|---|---|---|---|
| `<DEPENDENCY>` | 子元素 | 依赖容器，可出现多次（一个任务可依赖多个任务） | TBD |
| `<TASKID>` | 子元素 | 被依赖任务的 ID（同文件内） | TBD |
| `<DEPENDENCYTYPE>` | 子元素 | 依赖类型枚举 | TBD |

### 2.3 DEPENDENCYTYPE 枚举值（推测）

| 值 | 含义 | 说明 |
|---|---|---|
| `0` | Finish-to-Start (FS) | 前置任务完成后，后续任务才能开始 |
| `1` | Start-to-Start (SS) | 前置任务开始后，后续任务才能开始 |
| `2` | Finish-to-Finish (FF) | 前置任务完成后，后续任务才能完成 |
| `3` | Start-to-Finish (SF) | 前置任务开始后，后续任务才能完成 |

**验证状态**：以上为推测值，需真实样本确认。

### 2.4 多依赖示例（推测格式）

```xml
<TASK ID="3" TITLE="Task depending on multiple tasks" ...>
    <DEPENDENCY>
        <TASKID>1</TASKID>
        <DEPENDENCYTYPE>0</DEPENDENCYTYPE>
    </DEPENDENCY>
    <DEPENDENCY>
        <TASKID>2</TASKID>
        <DEPENDENCYTYPE>1</DEPENDENCYTYPE>
    </DEPENDENCY>
</TASK>
```

---

## 3. 跨文件依赖（不同文件间）

### 3.1 格式（TBD）

跨文件依赖可能需要引用外部文件中的任务。推测格式可能包含：

```xml
<DEPENDENCY>
    <FILENAME>other-tasks.xml</FILENAME>
    <TASKID>5</TASKID>
    <DEPENDENCYTYPE>0</DEPENDENCYTYPE>
</DEPENDENCY>
```

**验证状态**：完全推测，需真实样本确认。

### 3.2 解析策略

| 依赖类型 | 2.0 读取策略 | 2.0 写入策略 |
|---|---|---|
| 本地可解析 | 读取并建立引用 | 保持原样 |
| 本地不可解析（TASKID 不存在） | 读取为 unresolved | 保持原样，不删除 |
| 跨文件 | 读取为 external | 保持原样 |
| 格式未知 | 读取原始 XML | 保持原样 |

---

## 4. 当前版本处理状态

当前版本（`script.js`）**完全不处理**依赖关系：
- 不读取 `<DEPENDENCY>` 元素
- 不写入依赖信息
- 保存时由于保留原始 DOM 节点，依赖数据不会丢失（但如果创建新任务则不包含依赖）

---

## 5. 2.0 处理策略

### 5.1 Tier 分类

- **Tier A**：本地依赖的读写（M6 阶段实现）
- **Tier B**：跨文件依赖的保留（M6 阶段实现）
- **Tier C**：无

### 5.2 保留原则

1. 不理解的 `<DEPENDENCY>` 子元素结构必须完整保留
2. 无法解析的 TASKID 引用不得删除
3. 跨文件依赖在文件搬迁后必须保持原样（不自动修复路径）

---

## 6. Fixture 验证状态

| 类型 | Fixture 文件 | 验证状态 |
|---|---|---|
| 本地依赖 | `tests/fixtures/xml/dependencies/local-dependency.xml` | 已创建（模拟格式，需真实样本验证） |
| 跨文件依赖 | 无 | 待创建（需真实样本） |
| 不可解析依赖 | 无 | 待创建 |

---

## 7. 待确认事项

| 事项 | 状态 | 说明 |
|---|---|---|
| DEPENDENCY 元素实际结构 | TBD | 需真实 AbstractSpoon 样本 |
| DEPENDENCYTYPE 枚举值 | TBD | 需真实样本或文档 |
| 跨文件依赖格式 | TBD | 需真实样本 |
| 是否支持依赖循环 | TBD | 需确认 AbstractSpoon 行为 |
| 依赖的其他属性（如 lag time） | TBD | 需真实样本 |
