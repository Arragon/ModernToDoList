# 兼容性策略（COMPATIBILITY_POLICY）

> 对应任务：RD-M0-021 ~ RD-M0-024  
> 日期：2026-09-13

---

## 1. Semantic Lossless 精确定义（RD-M0-021）

### 1.1 定义

**Semantic Lossless**（语义无损）是指：

> 对 XML/TDL 文件执行"打开 → 任意编辑 → 保存"操作后，文件中所有**未被用户显式修改**的数据必须保持完全一致。

### 1.2 形式化表达

设：
- `F₀` = 原始文件字节序列
- `F₁` = 经过一次 no-op（打开→不编辑→保存）后的文件
- `P(F)` = 从文件 `F` 解析出的完整数据模型

Semantic Lossless 要求：

```
P(F₀) ≡ P(F₁)
```

即解析后的数据模型在语义上等价。

### 1.3 "等价"的精确含义

| 层面 | 要求 | 说明 |
|---|---|---|
| 业务字段 | 值完全相同 | TITLE、PRIORITY、PERCENTDONE 等 |
| 未知属性 | 完全保留 | 不认识的 TASK 属性不得删除或修改 |
| 未知元素 | 完全保留 | 不认识的子元素不得删除或修改 |
| 元素顺序 | 保持 | TASK 子元素的原始顺序不得改变 |
| 属性顺序 | 不要求 | XML 属性顺序不保证（XML 规范允许） |
| 空白/缩进 | 不要求 | 格式化差异可接受 |
| 编码 | 保持 | UTF-8/UTF-16/编码声明必须一致 |
| BOM | 保持 | 有 BOM 则保留 BOM，无 BOM 则不添加 |
| XML 声明 | 保持 | encoding 声明必须与实际编码一致 |

### 1.4 验收测试规则

**Golden Round-Trip Test**：

```
1. 读取 fixture 文件 F₀ 的字节
2. 解析为数据模型 M₀
3. 不做任何编辑
4. 将 M₀ 序列化回 XML → F₁
5. 解析 F₁ 为数据模型 M₁
6. 断言 M₀ ≡ M₁（语义等价）
```

**Single-Field Edit Test**：

```
1. 读取 fixture 文件 F₀
2. 解析为 M₀
3. 修改 M₀ 中的一个字段（如 TITLE）
4. 序列化为 F₁
5. 解析 F₁ 为 M₁
6. 断言：
   - 被修改的字段 = 新值
   - 所有其他字段 M₀[i] ≡ M₁[i]
```

---

## 2. Tier A/B/C 分类（RD-M0-022）

### 2.1 Tier A — 必须读写

2.0 必须完整支持读取和写入的字段。这些字段构成核心任务管理功能。

**规则**：
- 读取时必须正确解析
- 写入时必须正确更新
- 相关字段必须同步（如 LASTMOD 和 LASTMODSTRING）
- 新创建的任务必须包含所有 Tier A 字段的标准值

**Tier A 字段清单**：

| 字段 | 说明 |
|---|---|
| `ID` | 任务唯一标识（整数） |
| `TITLE` | 任务标题 |
| `PRIORITY` | 优先级 |
| `RISK` | 风险等级 |
| `PERCENTDONE` | 完成百分比 |
| `STARTDATE` / `STARTDATESTRING` | 开始日期 |
| `DUEDATE` / `DUEDATESTRING` | 截止日期 |
| `LASTMOD` / `LASTMODSTRING` | 最后修改时间 |
| `LASTMODBY` | 最后修改者 |
| `POS` | 位置排序 |
| `COMMENTSTYPE` | 注释类型（受保护） |
| `NEXTUNIQUEID` | 下一个唯一 ID |
| 子元素 `TASK` | 子任务 |
| 子元素 `FILEREFPATH` | 文件链接 |
| 子元素 `COMMENTS` | 注释内容 |
| 子元素 `CATEGORY` | 分类标签 |

### 2.2 Tier B — 应保留但暂不编辑

2.0 必须保留这些字段不被删除或修改，但第一版不提供编辑界面。

**规则**：
- 打开文件时必须读取并保留
- 保存时必须原样写回
- 不允许因为"不理解"而删除
- 新创建的任务应包含合理的默认值（如果该字段有标准默认值）

**Tier B 字段清单**：

| 字段 | 说明 |
|---|---|
| `PROJECTNAME` | 项目名称 |
| `FILENAME` | 原始文件名 |
| `FILEVERSION` | 文件格式版本号 |
| `APPVER` | 应用版本 |
| `FILEFORMAT` | 文件格式 |
| `REFID` | 引用 ID |
| `CREATIONDATE` / `CREATIONDATESTRING` | 创建日期 |
| `CREATEDBY` | 创建者 |
| 子元素 `METADATA` | 自定义元数据 |

### 2.3 Tier C — 只读保留

2.0 只读取和保留，完全不修改。

**规则**：
- 打开文件时读取
- 保存时原样写回
- 不提供编辑界面
- 不主动更新（即使逻辑上应该更新）

**Tier C 字段清单**：

| 字段 | 说明 |
|---|---|
| `EARLIESTDUEDATE` | 最早截止日期 |
| `SUBTASKDONE` | 子任务完成统计 |
| `TIMEESTIMATE` / `TIMEESTUNITS` | 时间估算 |
| `TIMESPENT` / `TIMESPENTUNITS` | 实际花费时间 |
| `TEXTCOLOR` / `TEXTWEBCOLOR` | 文本颜色 |
| `PRIORITYCOLOR` / `PRIORITYWEBCOLOR` | 优先级颜色 |
| `POSSTRING` | 位置字符串 |

---

## 3. 扩展字段命名空间策略（RD-M0-023）

### 3.1 问题

ModernToDoList 2.0 可能需要存储 AbstractSpoon TDL 标准中不存在的扩展数据（如 Smart Views 配置、内部状态等）。

### 3.2 策略

**优先使用 AbstractSpoon 原生字段**：
- 如果 AbstractSpoon 已有对应字段，必须使用原生字段
- 不允许为了方便而创建平行字段

**扩展字段命名规则**：

当确实需要存储扩展数据时，使用以下格式：

```xml
<METADATA ModernToDoList-EXT-GUID="value"/>
```

或使用自定义子元素：

```xml
<MODERNTODOLIST-EXT>
    <FIELD Name="..." Value="..."/>
</MODERNTODOLIST-EXT>
```

### 3.3 规则

1. **扩展字段不得干扰 AbstractSpoon 兼容性**：AbstractSpoon 打开含扩展字段的文件时不应报错
2. **扩展字段必须可识别**：使用明确的前缀（如 `ModernToDoList-EXT`）
3. **扩展字段不得存储在 Tier A/B/C 字段中**：不允许在标准属性中嵌入扩展数据
4. **扩展字段的保留与其他字段相同**：Semantic Lossless 规则同样适用

### 3.4 当前决策

M0 阶段不定义具体扩展字段。扩展字段的命名和格式将在实际需要时通过 ADR 确定。

---

## 4. Schema Audit 新增 Fixture 流程（RD-M0-024）

### 4.1 触发条件

当以下情况发生时，需要新增 fixture：

1. 发现新的 AbstractSpoon TDL 字段
2. 收到用户提供的真实 XML 样本
3. AbstractSpoon 发布新版本引入新字段
4. 测试中发现未覆盖的边界情况

### 4.2 流程

```
1. 获取新样本或发现新字段
   ↓
2. 分析字段格式和语义
   ↓
3. 确定 Tier 分类（A/B/C/TBD）
   ↓
4. 创建 fixture XML 文件
   - 放入对应子目录
   - 文件名描述场景
   ↓
5. 更新 TDL_FIELD_MAPPING.md
   - 添加新字段条目
   - 标注验证状态
   ↓
6. 计算 fixture SHA-256 hash
   - 更新 MANIFEST.sha256
   ↓
7. 编写对应的测试用例
   - parse 测试
   - round-trip 测试
   ↓
8. 提交 PR，包含：
   - fixture 文件
   - 文档更新
   - 测试代码
   - hash manifest 更新
```

### 4.3 规则

1. **Fixture 不可变**：一旦加入 manifest，fixture 文件不得修改。如需修改，创建新文件并更新 manifest。
2. **真实样本优先**：模拟格式的 fixture 必须标注"需真实样本验证"。
3. **每个 TBD 字段至少一个 fixture**：确保所有不确定字段都有测试覆盖。
4. **Hash 必须更新**：每次新增 fixture 必须更新 `MANIFEST.sha256`。

---

## 5. 兼容性验收清单

M0 阶段结束前必须确认：

- [ ] 所有 Tier A 字段有真实样本或明确格式定义
- [ ] 所有 Tier B 字段有真实样本
- [ ] 所有 Tier C 字段有真实样本
- [ ] TBD 字段已标注并记录原因
- [ ] Semantic Lossless 验收测试定义完成
- [ ] 扩展字段命名空间策略已定义
- [ ] Fixture 贡献流程已定义
- [ ] 所有 fixture 文件有 SHA-256 hash

---

## 6. 与 AbstractSpoon 的兼容性承诺

### 6.1 ModernToDoList 2.0 承诺

1. 打开 AbstractSpoon TDL 文件不丢数据
2. 保存后的文件可被 AbstractSpoon 正常打开
3. 不理解的字段完整保留
4. 编码格式不被改变
5. 注释类型不被静默修改

### 6.2 不承诺

1. 不承诺 100% UI 功能对等
2. 不承诺 AbstractSpoon 插件数据可编辑
3. 不承诺 RTF 注释可编辑
4. 不承诺与 AbstractSpoon 的实时同步

### 6.3 版本兼容矩阵

| 操作 | AbstractSpoon 创建的文件 | ModernToDoList 创建的文件 |
|---|---|---|
| AbstractSpoon 打开 | 原生支持 | 应能正常打开（需验证） |
| ModernToDoList 打开 | 应能正常打开 | 原生支持 |
| 交叉编辑后保存 | Semantic Lossless | Semantic Lossless |
