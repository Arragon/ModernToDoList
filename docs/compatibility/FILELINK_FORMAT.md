# 文件链接格式文档（FILELINK_FORMAT）

> 对应任务：RD-M0-010  
> 基线样本：`tests/fixtures/xml/real-world/london-underground.xml`  
> 日期：2026-09-13

---

## 1. 概述

AbstractSpoon ToDoList 使用 `<FILEREFPATH>` 子元素存储任务关联的文件链接。每个任务可以包含零个或多个文件链接。

---

## 2. 真实样本分析

### 2.1 test.xml 中的 FILEREFPATH

在 `london-underground.xml` 中，每个任务最多包含 **1 个** `<FILEREFPATH>`：

```xml
<TASK ID="1" TITLE="South Kensigton" ...>
    <FILEREFPATH>.\London Underground Photos\southken.jpg</FILEREFPATH>
    <METADATA .../>
</TASK>
```

路径特征：
- 使用 Windows 相对路径格式：`.\目录\文件名`
- 反斜杠 `\` 作为路径分隔符
- 以 `.\` 开头表示相对于 XML 文件所在目录

### 2.2 所有样本路径列表

| Task ID | Task Title | FILEREFPATH |
|---|---|---|
| 1 | South Kensigton | `.\London Underground Photos\southken.jpg` |
| 2 | Temple | `.\London Underground Photos\temple.jpg` |
| 3 | Westminster | `.\London Underground Photos\westmin.jpg` |
| 4 | Notting Hill Gate | `.\London Underground Photos\nottinghill.jpg` |
| 5 | Bank | `.\London Underground Photos\bank.jpg` |
| 6 | Edgware Road | `.\London Underground Photos\edgwareroad.jpg` |
| 7 | Farringdon | `.\London Underground Photos\farringdon.jpg` |
| 8 | Euston | `.\London Underground Photos\euston.jpg` |
| 9 | issue（task 5 的子任务） | 无 |

---

## 3. 路径格式

### 3.1 相对路径

```
.\目录\文件名
..\上级目录\文件名
```

- `.\` 前缀：相对于 XML 文件所在目录
- `..\` 前缀：相对于 XML 文件所在目录的上级目录

### 3.2 绝对路径

```
C:\Users\Username\Documents\file.pdf
\\server\share\file.pdf
```

- Windows 驱动器路径
- UNC 网络路径

### 3.3 URL

```
https://example.com/document.pdf
http://example.com/page.html
```

- 部分 AbstractSpoon 版本支持 URL 作为文件链接

---

## 4. 多文件链接

### 4.1 test.xml 行为

当前真实样本中每个任务只有 1 个 `<FILEREFPATH>`。

### 4.2 AbstractSpoon 支持

根据 AbstractSpoon 文档和行为，一个任务可以包含**多个** `<FILEREFPATH>` 子元素：

```xml
<TASK ID="1" TITLE="Multiple attachments" ...>
    <FILEREFPATH>.\docs\spec.pdf</FILEREFPATH>
    <FILEREFPATH>.\images\diagram.png</FILEREFPATH>
    <FILEREFPATH>C:\shared\report.xlsx</FILEREFPATH>
</TASK>
```

### 4.3 当前版本问题

当前 `parseTaskNode` 只读取**第一个** `<FILEREFPATH>`：

```javascript
} else if (child.tagName === 'FILEREFPATH') {
    fileRefPath = child.textContent;  // 后续会覆盖为最后一个
}
```

实际上由于遍历顺序，最终保留的是**最后一个** FILEREFPATH 的值。

**影响**：多文件链接场景下，只保留最后一个链接。

---

## 5. 当前版本处理状态

### 5.1 读取（parseTaskNode）

- 遍历所有子元素，找到 `<FILEREFPATH>`
- 只存储一个值（实际上是最后一个）
- 存储为 `task.fileRefPath`

### 5.2 写入（updateNodeFromTask）

- 查找现有 `<FILEREFPATH>` 节点
- 如果 `task.fileRefPath` 非空：创建或更新单个节点
- 如果 `task.fileRefPath` 为空：删除现有节点
- **不处理多个 FILEREFPATH 的情况**

### 5.3 数据丢失风险

- 打开含多个 FILEREFPATH 的文件 → 只读取最后一个 → 保存后前面的链接丢失
- 由于 DOM 节点保留机制，如果不编辑有 FILEREFPATH 的任务，原数据不会丢失

---

## 6. 2.0 处理策略

### 6.1 Tier 分类

- **Tier A**：FILEREFPATH 的完整读写，包括多个链接

### 6.2 处理规则

1. **读取所有 FILEREFPATH**：一个任务可能有零个或多个
2. **保持顺序**：写入时保持原始顺序
3. **路径格式保留**：不自动转换相对/绝对路径、不转换正反斜杠
4. **未知路径格式保护**：不认识的路径格式原样保留

### 6.3 路径解析

2.0 在需要打开文件时进行路径解析：

```
相对路径 → 基于 XML 文件所在目录解析
绝对路径 → 直接使用
URL → 使用默认浏览器打开
```

### 6.4 路径验证

2.0 不验证路径是否有效（文件是否存在），因为：
- 文件可能已被移动
- 网络路径可能暂时不可用
- 可移动存储可能未插入

---

## 7. Fixture 验证状态

| 类型 | Fixture 文件 | 验证状态 |
|---|---|---|
| 单个文件链接 | `tests/fixtures/xml/attachments/single-filelink.xml` | 已创建 |
| 多个文件链接 | `tests/fixtures/xml/attachments/multi-filelink.xml` | 已创建（模拟格式） |
| 真实样本 | `tests/fixtures/xml/real-world/london-underground.xml` | 已复制（每个任务 1 个链接） |

---

## 8. 待确认事项

| 事项 | 状态 | 说明 |
|---|---|---|
| AbstractSpoon 是否对 FILEREFPATH 数量有上限 | TBD | 需确认 |
| 是否支持其他类型的附件（如嵌入文件） | TBD | 需确认 AbstractSpoon 是否有其他附件元素 |
| URL 链接的实际表示 | TBD | 需确认是否直接存在 FILEREFPATH 中 |
