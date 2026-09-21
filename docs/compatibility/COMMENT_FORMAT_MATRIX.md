# 注释格式矩阵（COMMENT_FORMAT_MATRIX）

> 对应任务：RD-M0-012 ~ RD-M0-015  
> 日期：2026-09-13

---

## 1. COMMENTSTYPE 枚举值

AbstractSpoon ToDoList 使用 `COMMENTSTYPE` 属性标识注释内容的格式类型。

| COMMENTSTYPE 值 | 含义 | 内容格式 | test.xml 中是否存在 |
|---|---|---|---|
| `PLAIN_TEXT` | 纯文本 | 无格式文本，支持换行 | 是（所有任务） |
| `HTML` | HTML 富文本 | HTML 片段，通常包裹在 `<![CDATA[...]]>` 中 | 否 |
| `RTF` | RTF 富文本 | RTF 格式字符串 | 否 |
| `MARKDOWN` | Markdown | Markdown 格式文本 | 否（TBD） |

---

## 2. 读写策略矩阵

| COMMENTSTYPE | 2.0 读取策略 | 2.0 写入策略 | 说明 |
|---|---|---|---|
| `PLAIN_TEXT` | 完整读取 `<COMMENTS>` 文本内容 | 允许编辑，保持 `PLAIN_TEXT` 类型 | 最基础类型，必须完整支持 |
| `HTML` | 完整读取 `<COMMENTS>` 内容（含 CDATA） | **第一版只读**；后续版本可提供受限 HTML 编辑 | HTML 需要 sanitizer 防止 XSS |
| `RTF` | 完整读取 `<COMMENTS>` 内容 | **只读保留**；2.0 不提供 RTF 编辑 | RTF 解析复杂度高，不在 2.0 范围 |
| `MARKDOWN` | 完整读取 `<COMMENTS>` 内容 | **TBD**；需确认真实样本后决定 | 未在当前样本中观察到 |
| 未知/空值 | 完整读取 `<COMMENTS>` 内容 | **只读保留**；禁止修改类型 | 未知类型默认保护 |

---

## 3. 当前版本问题

### 3.1 COMMENTSTYPE 被静默覆盖

**问题代码**（`script.js` L868）：

```javascript
node.setAttribute('COMMENTSTYPE', 'PLAIN_TEXT');
```

**行为**：任何有 COMMENTS 内容的任务，保存时 COMMENTSTYPE 被强制设为 `PLAIN_TEXT`。

**影响**：
- 打开 HTML 注释文件 → 编辑无关字段 → 保存 → 注释类型被改为 PLAIN_TEXT
- HTML 标记可能被当作纯文本显示
- 数据格式隐性损失

**对应风险**：R-003

### 3.2 空 COMMENTS 处理

当前版本在写入空 COMMENTS 时会删除 `<COMMENTS>` 节点但不恢复原 COMMENTSTYPE。

---

## 4. Fixture 验证状态

| 类型 | Fixture 文件 | 验证状态 |
|---|---|---|
| PLAIN_TEXT | `tests/fixtures/xml/comments/plain-text-comment.xml` | 已创建（模拟格式） |
| HTML | `tests/fixtures/xml/comments/html-comment.xml` | 已创建（模拟格式，需真实样本验证） |
| RTF | 无 | 待创建（需真实样本） |
| MARKDOWN | 无 | TBD（需确认真实样本是否存在此类型） |

---

## 5. 2.0 注释处理规则

### 5.1 保护原则

1. **COMMENTSTYPE 是受保护属性**：除非用户明确执行类型转换，否则不得修改。
2. **未知类型默认只读**：不认识的 COMMENTSTYPE 值，整个 COMMENTS 字段设为只读。
3. **禁止静默转换**：不允许在保存时自动改变注释类型。

### 5.2 类型转换规则

如果未来支持类型转换（如 Plain → HTML），必须：
1. 用户显式触发转换操作
2. 显示转换预览
3. 用户确认后执行
4. 提供 Undo 支持

### 5.3 HTML 注释安全

HTML 注释写入时必须经过 sanitizer：
- 移除 `<script>` 标签
- 移除 `on*` 事件处理器
- 移除 `javascript:` URL
- 保留安全子集：`<p>`, `<b>`, `<i>`, `<u>`, `<br>`, `<ul>`, `<ol>`, `<li>`, `<a href>`

---

## 6. 待确认事项

| 事项 | 状态 | 说明 |
|---|---|---|
| RTF 注释的实际 XML 表示 | TBD | 需要真实 AbstractSpoon RTF 注释样本 |
| MARKDOWN 类型是否存在 | TBD | 需要确认 AbstractSpoon 是否支持此类型 |
| HTML CDATA vs 转义 | TBD | 需要确认 AbstractSpoon 使用 CDATA 还是实体转义 |
| 注释最大长度 | TBD | 需要确认是否有实际限制 |
