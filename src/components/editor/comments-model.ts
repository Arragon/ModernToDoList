/**
 * Frontend mirror of `src-tauri/src/domain/comments.rs` (RD-M7-014~018).
 *
 * The `COMMENTSTYPE` attribute is protected: it never changes as a side effect
 * of *viewing* content, and it changes only through the explicit,
 * confirmation-gated, undoable "Convert to Rich Text" action. Everything here
 * is a pure function — importing or calling it cannot mutate any task.
 */

/** Canonical `COMMENTSTYPE` attribute values written by AbstractSpoon TDL. */
export const ATTR_PLAIN_TEXT = "PLAIN_TEXT";
export const ATTR_HTML = "HTML";
export const ATTR_RTF = "RTF";

export type CommentsKind = "plain_text" | "html" | "rtf" | "unknown";

/**
 * The value of a task's `COMMENTSTYPE` attribute. Parsing is exact: only the
 * canonical upper-case spellings map onto a known kind; anything else is kept
 * verbatim in `raw` and treated as read-only.
 */
export interface CommentsType {
  kind: CommentsKind;
  /** Verbatim attribute value; only meaningful for `unknown`. */
  raw: string | null;
}

export type EditorMode = "plain_text" | "rich_text" | "read_only";

export function parseCommentsType(attr: string | null | undefined): CommentsType {
  if (attr === null || attr === undefined) return { kind: "plain_text", raw: null };
  switch (attr) {
    case ATTR_PLAIN_TEXT: return { kind: "plain_text", raw: attr };
    case ATTR_HTML: return { kind: "html", raw: attr };
    case ATTR_RTF: return { kind: "rtf", raw: attr };
    default: return { kind: "unknown", raw: attr };
  }
}

export function isEditable(type: CommentsType): boolean {
  return type.kind === "plain_text" || type.kind === "html";
}

export function isReadOnly(type: CommentsType): boolean {
  return type.kind === "rtf" || type.kind === "unknown";
}

/** Only `PLAIN_TEXT` payloads may be converted, and only with confirmation. */
export function canConvertToRichText(type: CommentsType): boolean {
  return type.kind === "plain_text";
}

export function editorModeFor(type: CommentsType): EditorMode {
  switch (type.kind) {
    case "plain_text": return "plain_text";
    case "html": return "rich_text";
    case "rtf":
    case "unknown": return "read_only";
  }
}

/** Type indicator shown next to the description (always shown for read-only). */
export function typeIndicator(type: CommentsType): string {
  switch (type.kind) {
    case "plain_text": return "Plain Text · PLAIN_TEXT";
    case "html": return "Rich Text · HTML";
    case "rtf": return "RTF · read-only";
    case "unknown": return `Unknown type "${type.raw ?? ""}" · read-only`;
  }
}

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Renders plain text as inert preview HTML (escaped, newlines become `<br>`). */
export function plainTextToPreviewHtml(text: string): string {
  if (text.length === 0) return "";
  const lines = text.split("\n").map((line, index) => {
    const cleaned = escapeHtml(line.replace(/\r$/, ""));
    return index === 0 ? cleaned : `<br>${cleaned}`;
  });
  return `<p>${lines.join("")}</p>`;
}

/**
 * Converts plain text into rich text for the explicit conversion action:
 * blank lines separate paragraphs, single newlines become `<br>`. Mirrors
 * `plain_text_to_html` in the domain layer (the caller still runs the result
 * through `sanitizeForCore` before it is offered to the backend).
 */
export function plainTextToHtml(text: string): string {
  let html = "";
  let paragraph: string[] = [];

  const flush = (): void => {
    if (paragraph.length === 0) return;
    html += `<p>${paragraph.map((line) => escapeHtml(line.trim())).join("<br>")}</p>`;
  };

  for (const rawLine of text.split("\n")) {
    const line = rawLine.replace(/\r$/, "");
    if (line.trim().length === 0) {
      flush();
      paragraph = [];
    } else {
      paragraph.push(line);
    }
  }
  flush();
  return html;
}

/**
 * Escaped excerpt of a read-only (RTF/unknown) payload, so the user can see
 * that content exists without the payload ever being interpreted as markup.
 */
export function readOnlyExcerpt(content: string): string {
  const trimmed = content.trim();
  const chars = Array.from(trimmed);
  if (chars.length <= 320) return trimmed;
  return `${chars.slice(0, 320).join("")}…`;
}

/**
 * Encode a payload for saving in the given mode (mirrors `encode_for_save`):
 * plain text is stored verbatim — escaping or sanitizing it would corrupt the
 * user's content; rich text goes through the commit sanitizer (done by the
 * caller via `sanitizeForCore`); read-only types can never be saved.
 */
export function canEncodeForSave(type: CommentsType): boolean {
  return isEditable(type);
}
