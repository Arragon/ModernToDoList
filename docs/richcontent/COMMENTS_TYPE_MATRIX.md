# M7 Rich Content — COMMENTSTYPE Preservation Matrix

> Scope: RD-M7-014 ~ RD-M7-018 (INH-1064), QA-M7-001 ~ QA-M7-006
> Implementation: `src-tauri/src/domain/comments.rs` (+ `domain::rich_text` for extraction)
> Tests: `#[cfg(test)] mod tests` in those files + `src-tauri/tests/m7_qa_tests.rs`
> Supersedes/extends: `docs/compatibility/COMMENT_FORMAT_MATRIX.md` §5
> Date: 2026-09-22

`COMMENTSTYPE` is a **protected attribute**. The 1.x bug this milestone exists to
fix was `script.js` unconditionally writing `COMMENTSTYPE="PLAIN_TEXT"` on save,
which silently downgraded every HTML comment in the file. The rules below are the
contract; every row is backed by a named test.

---

## 1. Parsing rule: exact match only

`CommentsType::from_attr_value` recognises **only** the canonical upper-case
spellings produced by AbstractSpoon TDL:

| Attribute value | Variant | Editable | Editor |
|---|---|---|---|
| `PLAIN_TEXT` | `CommentsType::PlainText` | yes | plain text |
| `HTML` | `CommentsType::Html` | yes | rich text (Tiptap) |
| `RTF` | `CommentsType::Rtf` | **no** | read-only |
| anything else (`MARKDOWN`, `html`, `Html`, `PLAIN TEXT`, `""`, …) | `CommentsType::Unknown(raw)` | **no** | read-only |
| attribute absent | `CommentsType::PlainText` (legacy default) | yes | plain text |

Anything that is not an exact canonical match is preserved **verbatim** in
`Unknown(String)` and `as_attr_value()` returns that same string, so the
attribute round-trips byte-for-byte. `html` (lower case) is therefore *not*
treated as HTML: guessing would rewrite the attribute on the next save.

`MARKDOWN` is called out as TBD in `docs/compatibility/COMMENT_FORMAT_MATRIX.md`.
Until a real sample is confirmed it lands in `Unknown`, i.e. read-only and
preserved — the safe default.

---

## 2. Behaviour matrix

| Behaviour | `PLAIN_TEXT` | `HTML` | `RTF` | Unknown | Absent |
|---|---|---|---|---|---|
| `editor_mode()` | `PlainText` | `RichText` | `ReadOnly` | `ReadOnly` | `PlainText` |
| `is_editable()` | true | true | false | false | true |
| `can_convert_to_rich_text()` | **true** | false | false | false | false |
| Editor payload (`projection.editor_payload`) | raw text | **sanitized** HTML (`sanitize_for_editor`) | `""` | `""` | `""` |
| Preview payload (`projection.display_html`) | escaped text, `\n`→`<br>` | **sanitized** HTML (`sanitize_for_preview`) | escaped excerpt in one `<p>` | escaped excerpt in one `<p>` | `""` |
| Type indicator | `纯文本 (PLAIN_TEXT)` | `富文本 (HTML)` | `RTF · 只读` | `未知类型 (X) · 只读` | `纯文本 (PLAIN_TEXT)` |
| Save path | stored **verbatim** | stored as **sanitized HTML** | `Err(ReadOnly)` | `Err(ReadOnly)` | see §3 |
| `COMMENTSTYPE` after save | unchanged | unchanged | n/a (refused) | n/a (refused) | n/a |
| Search text (`extract_plain_text`) | verbatim | sanitize → strip tags → `\n` block separators | `""` | `""` | `""` |
| UI placeholder for search | — | — | `[RTF 内容 · 只读，未纳入搜索索引]` | `[<TYPE> 内容 · 只读，未纳入搜索索引]` | — |
| XML write | `COMMENTSTYPE` = `PLAIN_TEXT` | `HTML` | `RTF` | original bytes | `PLAIN_TEXT` (legacy default) |

Plain text is stored **verbatim on purpose**. It is text, not markup: escaping or
sanitizing it at save time would corrupt the user's content and break the M2
byte-stability promise. The escaping happens on the *render* path
(`plain_text_to_preview_html`), so `<img src=x onerror=alert(1)>` typed into a
plain-text comment is displayed as inert text and never becomes an element.

---

## 3. The no-auto-create / no-auto-convert invariant (RD-M7-018)

> Opening, previewing, indexing or selecting a task must never create a
> `<COMMENTS>` element and never change `COMMENTSTYPE`.

Enforced structurally, not by convention:

* `project_for_view(comments: Option<&TaskComments>)` takes a **shared
  reference**. There is no code path from viewing to mutation.
* `CommentsField::view()` is the only viewing entry point on the stateful model
  and it increments `metrics.views` and nothing else.
* A projection of an absent payload carries `must_not_materialize: true`, which
  is the flag the IPC layer must honour when the inspector is opened.
* Saving an **empty** editor over an absent payload is a no-op: it returns
  `Ok(())` and leaves the value `None`. No `COMMENTSTYPE` churn, no empty
  `<COMMENTS>` node appearing in the XML.
* `convert_to_rich_text` on an absent payload returns `Err(NothingToConvert)`
  rather than materializing `COMMENTSTYPE="HTML"` on a task that has no comment.
* A comment **is** created when the user actually types the first characters
  (`save` with non-empty content over `None`). That is an explicit edit, not a
  side effect of viewing, and the caller must pass the target type explicitly
  (`default_new_type`), which is only consulted in that one case.

Proof: `qa_m7_006_viewing_never_creates_or_converts_comments` performs 100 views
of an empty field and 25 views of each stored type, interleaving
`extract_plain_text`, `extract_search_text`, `project_for_view` and
`opaque_placeholder`, then asserts the value is byte-identical and that
`type_changes`, `materializations` and `saves` are all zero. The same invariant
is re-tested per type in `comments::tests::viewing_never_creates_or_converts_comments`.

---

## 4. Conversion: "Convert to Rich Text" (RD-M7-016)

`PLAIN_TEXT → HTML` only, and only through an explicit user action.

```
CommentsField::convert_to_rich_text(confirmed: bool)
    confirmed == false  ->  Err(ConfirmationRequired)     // nothing changes
    current == None     ->  Err(NothingToConvert)
    current == Html     ->  Err(AlreadyRichText)
    current == Rtf      ->  Err(ReadOnly("RTF"))
    current == Unknown  ->  Err(ReadOnly(raw))
    current == PlainText->  Ok(CommentsConversion { before, after, confirmed: true })
```

Content transformation (`plain_text_to_html`):

* lines are HTML-escaped, then grouped: a blank line ends a paragraph, single
  newlines inside a paragraph become `<br>`;
* the result is passed through `sanitize_for_core`, so the bytes that get
  persisted are policy-clean by construction;
* CJK is unaffected: `"第一行\n第二行\n\n第二段"` →
  `<p>第一行<br>第二行</p><p>第二段</p>`.

**Reversibility.** `CommentsConversion` keeps the exact `before` payload, and
`undo_conversion(&conversion)` restores it byte-for-byte, including the
`COMMENTSTYPE` value. `metrics.type_changes` is decremented, so the invariant
"the type changed N times" stays truthful across undo. `qa_m7_005` also re-runs
the conversion after an undo and asserts it produces identical bytes.

There is deliberately **no** automatic upgrade path. In particular the UI must
not upgrade a plain-text comment just because the rich-text editor was mounted
(architecture doc §22.3: 不允许打开纯文本后因为 UI 使用 Tiptap 就自动变 HTML).

---

## 5. Read-only types: `RTF` and Unknown

* No editor is opened (`editor_payload` is empty), so there is no way to type
  into the payload.
* The preview shows a **320-char escaped excerpt** inside a single `<p>` plus the
  type indicator, so the user can see that content exists without the app
  pretending to render it. The excerpt is escaped, and `sanitize_indicator`
  strips `< > " &` from the raw type string before it is echoed into UI text.
* `save` returns `Err(ReadOnly)` and increments `metrics.rejections`; the stored
  value is untouched.
* `convert_to_rich_text` returns `Err(ReadOnly)`.
* RTF is never parsed. Extracting text from RTF properly is out of M7 scope;
  `extract_plain_text` returns `""` so the payload cannot pollute the FTS index
  with control words, and `opaque_placeholder` explains why.

---

## 6. XML mapping contract

The M2 lossless core owns the XML. M7 supplies the value object and the exact
attribute string:

```rust
// read (mappers::read_task, COMMENTS branch) — must be CDATA-aware, see §7
task.comments = Some(TaskComments::from_attr(elem.get_attr("COMMENTSTYPE"), payload));

// write (mappers::write_task)
if let Some(c) = &task.comments {
    elem.set_attr("COMMENTSTYPE", c.attr_value());   // exact original bytes
}
```

`CommentsField::xml_attr_value()` returns `PLAIN_TEXT` when there is no payload,
mirroring the pre-existing `task::CommentType::default()` behaviour so documents
without comments serialize exactly as they do today.

---

## 7. Known gap in the current read path (must be fixed by the owner of `mappers.rs`)

`XmlElement::text_content()` concatenates **only** `XmlNode::Text` children, so a
CDATA-wrapped payload reads back as an empty string:

```xml
<COMMENTS><![CDATA[<html><body><p>…</p></body></html>]]></COMMENTS>
```

TDL writes HTML comments in exactly this form (see
`tests/fixtures/xml/comments/html-comment.xml` and
`tests/fixtures/xml/richtext/html-legacy-cdata.xml`). `mappers::read_task`
currently uses `child.text_content()`, so an HTML comment is read as empty and
would be **written back empty** — silent data loss, the same class of bug as the
`COMMENTSTYPE` overwrite this milestone fixes.

The round trip itself is safe (`xml_parser` keeps `XmlNode::CData` and
`xml_serializer` re-emits it); only the domain mapping drops it. Required fix, in
the `COMMENTS` arm of `mappers::read_task`:

```rust
"COMMENTS" => {
    let mut payload = String::new();
    for node in &child.children {
        match node {
            XmlNode::Text(t) | XmlNode::CData(t) => payload.push_str(t),
            _ => {}
        }
    }
    task.comments = Some(TaskComments::from_attr(elem.get_attr("COMMENTSTYPE"), payload));
}
```

`m7_qa_tests::qa_m7_002_xml_html_cdata_payload_extraction` pins the expected
behaviour with a CDATA-aware helper, so it will pass before and after the fix and
documents the intended semantics.

---

## 8. API reference

| Item | Purpose |
|---|---|
| `CommentsType` | the protected attribute value; `from_attr_value` / `from_attr_opt` / `as_attr_value` |
| `EditorMode` | `PlainText` / `RichText` / `ReadOnly` — drives UI editor selection |
| `TaskComments` | `{ comments_type, content }` value object |
| `project_for_view(Option<&TaskComments>) -> CommentsProjection` | read-only projection; cannot mutate |
| `CommentsProjection` | `mode`, `editable`, `convertible_to_rich_text`, `type_indicator`, `editor_payload`, `display_html`, `display_text`, `byte_length`, `must_not_materialize` |
| `CommentsField` | stateful model enforcing the matrix; `view`, `save`, `convert_to_rich_text`, `undo_conversion`, `xml_attr_value`, `metrics` |
| `CommentsMetrics` | `views`, `saves`, `rejections`, `type_changes`, `materializations`, `conversion_undos` |
| `encode_for_save(&CommentsType, &str)` | per-mode save encoding; refuses read-only types |
| `plain_text_to_html` / `plain_text_to_preview_html` | conversion and inert preview rendering |
| `CommentsError` | `ReadOnly`, `ConfirmationRequired`, `AlreadyRichText`, `NothingToConvert`, `UnsupportedTarget`, `Rejected` |
| `rich_text::extract_plain_text` / `extract_search_text` / `extract_plain_text_by_attr` | RD-M7-028 search extraction |

All public types derive `Serialize`/`Deserialize` (and the error enums too), so
they can cross the Tauri IPC boundary directly.

---

## 9. Test coverage map

| Test | Row(s) covered |
|---|---|
| `qa_m7_001_plain_text_type_is_preserved_on_save` | PLAIN_TEXT save, wrong `default_new_type` ignored, inert preview |
| `qa_m7_001_xml_plain_text_comment_roundtrip` | PLAIN_TEXT through the real XML core, incl. a task with no comments |
| `qa_m7_002_html_type_is_preserved_and_content_sanitized` | HTML save + sanitize + alias folding |
| `qa_m7_002_xml_html_cdata_payload_extraction` | CDATA-wrapped HTML, legacy `<b>`/`<i>`, CJK |
| `qa_m7_003_rtf_is_read_only_with_type_indicator` | RTF read-only, indicator, no convert, no index text |
| `qa_m7_004_unknown_commentstype_is_preserved_and_read_only` | `MARKDOWN`, `html`, `Html`, `PLAIN TEXT`, `""`, `SOMETHING_ELSE` |
| `qa_m7_004_xml_opaque_commentstypes_are_preserved` | byte-exact attribute round trip + unknown attr/element/comment survival |
| `qa_m7_005_convert_to_rich_text_is_confirmed_and_undoable` | confirmation gate, single type change, byte-exact undo, redo stability |
| `qa_m7_006_viewing_never_creates_or_converts_comments` | RD-M7-018 invariant, all types |
| `qa_m7_006_search_text_extraction_matrix` | RD-M7-028 across all types incl. CJK |
| `comments::tests::*` (22 tests) | unit-level matrix, escaping, metrics |
