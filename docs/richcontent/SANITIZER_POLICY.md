# M7 Rich Content — HTML Sanitizer Policy

> Scope: RD-M7-011 ~ RD-M7-013 (INH-1063), QA-M7-007 ~ QA-M7-009
> Implementation: `src-tauri/src/domain/sanitizer.rs`
> Tests: `#[cfg(test)] mod tests` in that file + `src-tauri/tests/m7_qa_tests.rs`
> Date: 2026-09-22

This is a **security control**, not a formatter. Its job is to make it
impossible for stored or pasted rich text to execute script in the WebView,
while preserving as much benign formatting as possible and keeping the output
byte-stable (the result is persisted into a TDL XML document that must round-trip
losslessly).

---

## 1. Design: allow-list re-serialization

The sanitizer does **not** scrub strings. It:

1. Parses the untrusted fragment with `html5ever` (through the `scraper` crate),
   i.e. with the same spec-compliant tokenizer the WebView2/Chromium renderer
   uses. This removes the "sanitizer and browser disagree about where a tag
   starts" bug class.
2. Walks the resulting tree and emits a **brand-new string**. Only whitelisted
   elements with whitelisted, re-validated attributes are ever written. Anything
   else never reaches the output.
3. Re-escapes every text node and attribute value on the way out, so text can
   never re-materialize as markup (mutation-XSS / double-escaping defence).

Consequences that are load-bearing for the rest of the system:

| Property | Guarantee |
|---|---|
| Determinism | Attributes are emitted **sorted by name**. `scraper` stores them in a hash map, so iterating directly would make the persisted bytes vary between runs and would produce spurious "file changed" fingerprints. |
| Idempotency | `sanitize(sanitize(x)) == sanitize(x)` for every tested input. Re-feeding stored content is always a no-op. |
| Case folding | Tag and attribute names are lower-cased by the parser, so `<SCRIPT>` and `ONERROR` are handled identically to their lower-case forms. |
| Duplicate attributes | html5ever keeps the first and drops later duplicates, so `<a href="safe" href="javascript:…">` cannot smuggle a second value. |
| Failure mode | Oversize input is **refused** (`SanitizeError::InputTooLarge`), never truncated into a half-sanitized document. |

---

## 2. The four choke points (RD-M7-013)

Each is a separate named entry point with its own [`SanitizeContext`], so an
audit can tell which path produced a given string, and so each path can be
tightened independently.

| Entry point | Context | Caller | Extra behaviour |
|---|---|---|---|
| `sanitize_paste_import` | `PasteImport` | Clipboard / drag-drop HTML entering the app | Captures `data:` image payloads into `outcome.inline_images` and leaves a placeholder `src` for the asset service to fill in |
| `sanitize_for_editor` | `CoreToEditor` | Core → Editor feed | Defence in depth: XML edited by AbstractSpoon TDL or by hand is never trusted just because we wrote it |
| `sanitize_for_core` | `EditorToCore` | Editor → Core commit | Produces exactly the bytes that get persisted into `<COMMENTS>` |
| `sanitize_for_preview` | `PreviewRender` | Read-only preview render | Accepts an optional `src_rewrite` hook so a managed reference can be turned into a WebView-loadable URL |

All four apply the **same whitelist**. They differ only in the knobs above; none
of them is a "weaker" pass. QA-M7-009 chains all four in pipeline order and
asserts the result converges and stays converged.

---

## 3. Allow-list

### 3.1 Elements

```
p  h1  h2  h3  h4  h5  h6  strong  em  u  s  span
ul  ol  li  blockquote  a  code  pre  img  br
```

Void (serialized without a closing tag): `br`, `img`.

The architecture document (§22.2) lists `h1-h4`; the M6-M10 delivery plan
(§2.2) lists `h1-h6`. This implementation follows the **delivery plan** and
allows `h1-h6`. Tightening to `h4` is a one-line change to `ALLOWED_TAGS`.

### 3.2 Legacy aliases (folded, not flattened)

Existing TDL HTML comments use authoring spellings that are not on the
whitelist. Rather than destroying the formatting, they are folded onto the
nearest whitelisted element:

| Source | Emitted |
|---|---|
| `b` | `strong` |
| `i` | `em` |
| `strike`, `del` | `s` |
| `ins` | `u` |
| `tt` | `code` |
| `big`, `small`, `font` | `span` |
| `center` | `p` |

Any other unknown-but-well-formed element (`div`, `section`, `figure`, …) is
**unwrapped**: no tag is emitted, its children are kept.

### 3.3 Attributes

| Element | Allowed attributes |
|---|---|
| `a` | `href`, `class`, `style` |
| `img` | `src`, `alt`, `width`, `height`, `class`, `style` |
| `ol` | `start`, `class`, `style` |
| everything else | `class`, `style` |

Every other attribute is dropped and reported. In particular:

* **All `on*` attributes are dropped and reported as `EventHandler`** — including
  on elements that are dropped or unwrapped wholesale (`record_event_handlers`),
  so the audit trail never under-reports the most common attack attribute.
* **Namespaced attributes are dropped** (`xlink:href` and friends). `scraper`
  exposes only local names through `Element::attrs()`, so the raw `QualName`
  map is inspected to detect a prefix.

### 3.4 `href` policy — http/https only

`sanitize_url` implements the URL normalization a browser performs *before*
resolution, then applies a strict scheme allow-list:

1. Strip leading/trailing C0-control-or-space and DEL.
2. Remove every ASCII tab, LF and CR from the interior.
3. Reject if a backslash remains (browsers map `\` onto `/`, which turns
   `https:/\evil.com` into a protocol-relative URL).
4. Reject if any C0 control or DEL remains.
5. Take the token before the first `:` that precedes any `/`, `?`, `#`. It must
   match `[A-Za-z][A-Za-z0-9+.-]*`.
6. Accept only `http` / `https` (compared case-insensitively), and require the
   `//` authority that follows.
7. Everything else is rejected: `javascript:`, `vbscript:`, `data:`, `file:`,
   `blob:`, `about:`, `ms-msdt:`, `search-ms:`, unknown schemes, relative URLs
   and protocol-relative (`//host/…`) URLs.

Defeated by construction: `JAVASCRIPT:`, `JaVaScRiPt:`, `java\tscript:`,
`java\nscript:`, `java\rscript:`, `" javascript:"`, `"\u{0}javascript:"`,
`&#106;avascript:`, `&#x6a;avascript:`, `&#0000106;avascript:`,
`javascript&#58;`. Character references are decoded by the parser before the
value is ever seen, so entity-encoded schemes are caught by the same code path.

**Deliberate restriction:** `mailto:` and `tel:` are rejected. The delivery plan
specifies "http/https only". Adding them means editing one match arm in
`sanitize_url`; do not do it silently.

### 3.5 `src` policy — managed relative paths only

`sanitize_image_src` accepts exactly:

```
(../){0,4} .assets / <document-id> / images / <stem> . <ext>
```

* `<document-id>` — `[A-Za-z0-9._-]{1,128}`, no `.`/`..`; when
  `SanitizeOptions::managed_document_id` is set it must match exactly, which
  stops one document's rich text from hot-linking another document's assets.
* `<stem>` — `[A-Za-z0-9_-]{1,100}` (a UUID in practice).
* `<ext>` — one of `png jpg jpeg gif webp bmp`, lower case.
* Rejected outright: any scheme (`data:`, `http:`, `file:`, `javascript:`),
  absolute paths, protocol-relative paths, backslashes, percent-encoding
  (`%2e%2e%2f`), interior whitespace or control characters, more than four `../`
  prefixes, and anything not matching the four-segment layout.

Interior whitespace is rejected rather than stripped: silently rewriting
`doc 1` into `doc1` would redirect the reference at a *different* document
directory.

**`svg` is not an allowed image type.** SVG is an XML scripting host; allowing
it would re-open the entire XSS surface through a picture.

### 3.6 `style` policy — mini CSS sanitizer

`sanitize_style` splits on `;`, then per declaration:

* Property (lower-cased) must be in `ALLOWED_STYLE_PROPERTIES`:
  `background-color, color, font-size, font-style, font-weight, height,
  letter-spacing, line-height, text-align, text-decoration,
  text-decoration-line, vertical-align, width`.
* Value must be ≤ 64 chars and use only `[A-Za-z0-9 #%.,-_()+/ ]`.
* Value must not contain `url(`, `url (`, `expression`, `javascript`,
  `vbscript`, `behavior`, `-moz-binding`, `@import`, `/*`, `*/`.
* A malformed declaration, an unsafe value, more than 16 declarations or a
  result longer than 512 chars rejects the **whole attribute** (fail closed).
* Surviving declarations are re-emitted sorted by property, so output is
  deterministic regardless of source order.

`font-family` is deliberately excluded: its legitimate values need quotes, and
quotes are exactly what makes attribute-injection payloads hard to review.

### 3.7 `class` and text attributes

* `class`: tokens must match `[A-Za-z0-9_-]{1,64}`, max 8 tokens, order
  preserved, duplicates removed. Invalid tokens are dropped; if nothing
  survives the attribute is dropped.
* `alt`: control characters stripped, capped at 512 chars, escaped on output.
* `width` / `height` / `start`: digits with an optional `px` suffix only.
  `100%`, `auto` and `expression(...)` are refused.

---

## 4. Drop-with-subtree list

These elements are removed **together with all descendants**. Their children are
never unwrapped.

```
script style iframe object embed applet frame frameset noframes noembed
noscript template xmp plaintext listing svg math base meta link title
form input button select textarea option optgroup fieldset output
audio video source track canvas dialog slot portal marquee blink keygen
param isindex
```

Two families live here, and the distinction matters:

1. **Actively dangerous hosts** — `script`, `iframe`, `object`, `embed`, …
2. **Raw-text / escapable-raw-text / deferred-content hosts** — `noscript`,
   `template`, `style`, `title`, `textarea`, `xmp`, `plaintext`. Their children
   are *not* ordinary nodes. Whether they are parsed as markup depends on the
   scripting flag and on the parser's insertion mode, so unwrapping them is the
   classic mutation-XSS pivot
   (`<noscript><p title="</noscript><img src=x onerror=alert(1)>">`).
   Dropping the subtree removes the parser-differential class entirely.
3. **Foreign namespaces** — any element whose namespace is not
   `http://www.w3.org/1999/xhtml` (SVG, MathML, `annotation-xml`) is dropped
   with its subtree, independent of its local name.

`html`, `head` and `body` are **not** on this list. `scraper::Html::parse_fragment`
builds the skeleton `Fragment → html → {head, body}` and parses the payload into
`body`; dropping those names would discard the entire document. They are not
whitelisted, so they are unwrapped — no tag and no attributes are emitted, which
also covers a literal `<body onload=…>` wrapper in a legacy comment.

### 4.1 Malformed tag names: unwrap, do not drop

A probe such as `<scr<script>ipt>` tokenizes to an element literally named
`scr<script` that is **never closed**, so the entire remainder of the fragment
becomes its descendant. Dropping that subtree would let a 12-byte
attacker-controlled prefix silently delete the rest of the user's comment — an
attacker-triggered data-loss bug in a product whose core promise is data
preservation.

The element is therefore **unwrapped**: reported as `MalformedTagName` (a
threat), no tag emitted, children kept, all text escaped. The residue is inert
(`ipt&gt;alert(1)`) and a second pass over the output finds zero threats.

---

## 5. Inline images: base64 is never persisted

RD-M7-023 forbids base64 in the XML by default.

* `sanitize_for_core` / `sanitize_for_editor` / `sanitize_for_preview`:
  a `data:` URI `src` is dropped, and an `<img>` left without a valid `src` is
  removed entirely. No base64 can reach the document.
* `sanitize_paste_import`: the payload is **captured** into
  `SanitizeOutcome::inline_images` (`mime`, `base64`, `alt`, `slot`) and the
  `<img>` keeps a placeholder `src` of the form `{{m7-inline-image:<n>}}`.
  `ImageAssetStore::externalize_pasted_html` stores the bytes under
  `.assets/<doc>/images/<uuid>.<ext>` and splices the managed reference in.
* The placeholder is deliberately **not** a valid managed reference. If a caller
  forgets to replace it, the next sanitization pass drops the `<img>` instead of
  persisting a broken or exploitable reference. Fail closed.
* Only `image/png|jpeg|gif|webp|bmp` with `;base64` are captured. `data:text/html`
  and `image/svg+xml` are refused.

---

## 6. Limits

| Limit | Value | Constant |
|---|---|---|
| Input size | 4 MiB | `DEFAULT_MAX_INPUT_BYTES` |
| URL length | 2048 | `MAX_URL_LEN` |
| Text attribute (`alt`) | 512 chars | `MAX_TEXT_ATTR_LEN` |
| Single CSS value | 64 chars | inline |
| Total `style` | 512 chars / 16 declarations | inline |
| `class` tokens | 8 × 64 chars | inline |
| Managed `../` depth | 4 | `MAX_MANAGED_SRC_DEPTH` |

---

## 7. Also provided

* `html_to_plain_text(html) -> String` — search-index extraction (RD-M7-028).
  Dangerous subtrees contribute nothing; block elements separate with a single
  `\n`; NBSP becomes a space; CJK text is passed through untouched (no spaces
  inserted between Chinese characters, so `任务描述` stays one searchable token).
* `sanitize_image_src`, `managed_image_reference`, `validate_asset_file_name`,
  `is_safe_path_segment` — shared with the asset store so that reference
  *generation* and reference *validation* can never drift apart.
* `Removal` / `RemovalReason` / `SanitizeOutcome::threat_count()` — a machine
  readable audit trail, so the UI can tell the user that content was stripped
  and QA can assert on it.

---

## 8. Known limitations (accepted, documented)

1. **No `url()` in CSS at all** — not even for managed asset paths. Background
   images are out of M7 phase-1 scope.
2. **`mailto:` / `tel:` links are rejected.** See §3.4.
3. **Unicode bidi and homoglyph spoofing** in link *text* is not addressed:
   `<a href="https://good.example">https://еvil.example</a>` (Cyrillic `е`) is
   perfectly safe HTML and perfectly misleading to a human. That is a UI
   concern (show the resolved href in the status bar / on hover).
4. **No table support** — `table/tr/td/th` are unwrapped, per the phase-1 scope
   freeze in the architecture document §22.1.
5. **IRIs are passed through** after control-character rejection; they are not
   percent-encoded. Browsers do the encoding at navigation time.
6. **Parser-version coupling.** The guarantees in §1 and §4 rely on html5ever
   0.27 semantics via `scraper` 0.20. Bumping `scraper` must re-run
   `qa_m7_007`/`qa_m7_008`/`qa_m7_009` before it ships.

---

## 9. Test coverage map

| Test | Covers |
|---|---|
| `sanitizer::tests::mixed_case_and_whitespace_obfuscation_is_defeated` | case folding, unquoted attributes |
| `sanitizer::tests::javascript_scheme_variants_are_rejected` | 15 scheme-obfuscation payloads |
| `sanitizer::tests::entity_encoded_scheme_is_decoded_then_rejected` | `&#106;avascript:` |
| `sanitizer::tests::nested_script_probe_cannot_produce_an_element`, `raw_text_hosts_are_dropped_not_unwrapped` | tokenizer probes, mXSS pivots |
| `sanitizer::tests::svg_and_mathml_subtrees_are_dropped` | foreign namespaces |
| `sanitizer::tests::sanitization_is_idempotent`, `double_escaped_text_never_becomes_markup` | mutation-XSS, stability |
| `sanitizer::tests::attribute_order_is_deterministic` | byte-stable output |
| `sanitizer::tests::src_must_be_a_managed_relative_path`, `managed_document_id_is_enforced_end_to_end` | asset path policy |
| `sanitizer::tests::style_attribute_is_limited_and_canonicalized` | CSS mini-sanitizer |
| `sanitizer::tests::four_entry_points_are_distinct_and_all_safe` | RD-M7-013 |
| `m7_qa_tests::qa_m7_007_*` | 40-payload XSS matrix + benign-formatting survival |
| `m7_qa_tests::qa_m7_008_*` | 36-entry dangerous-URL matrix + `src` matrix |
| `m7_qa_tests::qa_m7_009_*` | four choke points, pipeline convergence |
| `m7_qa_tests::qa_m7_007_xml_hostile_comment_is_neutralized_before_render` | hostile payload stored in a real TDL fixture |
