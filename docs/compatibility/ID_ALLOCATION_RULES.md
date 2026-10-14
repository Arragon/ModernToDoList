# ID 分配规则（ID_ALLOCATION_RULES）

> 对应任务：RD-M0-017  
> 基线样本：`tests/fixtures/xml/real-world/london-underground.xml`  
> 日期：2026-09-13

---

## 1. 概述

AbstractSpoon ToDoList 使用整数 ID 作为任务的唯一标识符。ID 在文件范围内唯一，由 `NEXTUNIQUEID` 属性控制分配。

---

## 2. 真实样本分析

### 2.1 TODOLIST 根元素

```xml
<TODOLIST ... NEXTUNIQUEID="10" ...>
```

- `NEXTUNIQUEID="10"`：下一个可分配的 ID 值为 10

### 2.2 TASK ID 分布

| Task ID | Title | 层级 |
|---|---|---|
| 1 | South Kensigton | 根 |
| 2 | Temple | 根 |
| 3 | Westminster | 根 |
| 4 | Notting Hill Gate | 根 |
| 5 | Bank | 根 |
| 6 | Edgware Road | 根 |
| 7 | Farringdon | 根 |
| 8 | Euston | 根 |
| 9 | issue | 嵌套在 Task 5 下 |

**观察**：
- ID 为连续整数：1-9
- NEXTUNIQUEID=10，恰好是 max(ID)+1
- 嵌套任务（ID=9）与根任务共享同一 ID 空间
- 删除任务后 ID 不回收（推测）

---

## 3. ID 格式规则

### 3.1 AbstractSpoon TDL 格式

- **类型**：正整数
- **范围**：`1` 到 `2^31-1`（推测为 32 位有符号整数）
- **文件内唯一**：同一 XML 文件内 ID 不得重复
- **跨文件不要求唯一**：不同文件的 ID 空间独立

### 3.2 当前版本问题

当前 `script.js` 生成的 ID 格式：

```javascript
const generateId = () => {
    return Date.now().toString(36) + Math.random().toString(36).substr(2, 5);
};
```

**示例输出**：`"m5x8k2p9q"`（base36 字符串）

**与 AbstractSpoon 的不兼容**：
1. 使用 base36 字符串而非整数
2. AbstractSpoon 打开含此类 ID 的文件可能报错或行为异常
3. 当前 `createNewTaskObject` 中 `node.setAttribute('ID', id)` 写入的是字符串

---

## 4. NEXTUNIQUEID 语义

### 4.1 定义

`NEXTUNIQUEID` 是 TODOLIST 根元素属性，表示下一个可分配的任务 ID 值。

### 4.2 分配规则

```
新任务 ID = NEXTUNIQUEID
NEXTUNIQUEID = NEXTUNIQUEID + 1
```

### 4.3 真实样本验证

```
max(所有任务 ID) = 9
NEXTUNIQUEID = 10
关系：NEXTUNIQUEID = max(ID) + 1 ✓
```

### 4.4 删除后行为（推测）

- 删除任务后，其 ID **不回收**
- NEXTUNIQUEID 不回退
- 确保历史引用（如依赖关系）不会指向新创建的任务

---

## 5. 2.0 ID 分配策略

### 5.1 Tier 分类

- **Tier A**：NEXTUNIQUEID 的正确维护

### 5.2 分配算法

```rust
fn allocate_task_id(doc: &mut XmlDocument) -> u32 {
    let next_id = doc.root.next_unique_id;
    doc.root.next_unique_id = next_id + 1;
    next_id
}
```

### 5.3 规则

1. **新任务 ID 必须来自 NEXTUNIQUEID**：不允许使用随机值、时间戳或其他生成方式
2. **分配后必须递增 NEXTUNIQUEID**：确保下次分配不冲突
3. **ID 一旦分配不得回收**：即使任务被删除
4. **打开文件时验证**：
   - 扫描所有任务 ID
   - 如果 `max(ID) >= NEXTUNIQUEID`，修正 `NEXTUNIQUEID = max(ID) + 1`
   - 记录警告日志
5. **ID 冲突处理**：
   - 如果发现重复 ID，为后出现的任务重新分配 ID
   - 更新相关依赖引用

### 5.4 导入兼容

如果打开的文件包含非整数 ID（如当前版本生成的 base36 字符串）：

1. 尝试将 ID 解析为整数
2. 如果解析失败，为所有非整数 ID 的任务重新分配整数 ID
3. 更新文件内所有引用（依赖、跨任务引用等）
4. 更新 NEXTUNIQUEID
5. 记录数据迁移日志

---

## 6. REFID 字段

### 6.1 定义

`REFID` 是 TASK 属性，在 test.xml 中所有任务的值均为 `"0"`。

### 6.2 推测用途

- 可能用于跨文件引用
- 可能用于标识原始任务（复制后保持引用）
- `"0"` 可能表示"无引用"

### 6.3 处理策略

- **Tier B**：保留但不编辑
- 不修改已有 REFID 值
- 新任务的 REFID 设为 `"0"`

---

## 7. Fixture 验证状态

| 场景 | 覆盖文件 | 状态 |
|---|---|---|
| 正常 ID 分配 | `real-world/london-underground.xml` | 已验证（ID 1-9, NEXTUNIQUEID=10） |
| 空任务列表 | `canonical/empty-todolist.xml` | NEXTUNIQUEID=1 |

---

## 8. 待确认事项

| 事项 | 状态 | 说明 |
|---|---|---|
| ID 最大值 | TBD | 需确认是否为 32 位整数上限 |
| ID 溢出行为 | TBD | NEXTUNIQUEID 达到上限后如何处理 |
| REFID 的实际用途 | TBD | 需更多样本或文档确认 |
| 跨文件 ID 引用机制 | TBD | 需确认 AbstractSpoon 如何实现 |
