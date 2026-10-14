/**
 * Client-side mirror of the Rust whitelist sanitizer
 * (`src-tauri/src/domain/sanitizer.rs`, RD-M7-011~013).
 *
 * The backend owns the authoritative policy; this module reproduces the exact
 * same whitelist so the editor can enforce it at the four choke points without
 * a round trip:
 *
 *   1. paste import      — clipboard HTML entering the editor
 *   2. Core → Editor     — stored content being fed into Tiptap
 *   3. Editor → Core     — editor output being committed via `update_task_field`
 *   4. preview render    — content rendered read-only through `v-html`
 *
 * Rules mirrored from Rust (keep the two files in sync):
 *   - allowed tags: p, h1-h6, strong, em, u, s, span, ul, ol, li, blockquote,
 *     a, code, pre, img, br
 *   - allowed attributes: class + style everywhere, href on `a`,
 *     alt/src/width/height on `img`, start on `ol`
 *   - href: http/https only; img src: relative managed
 *     `.assets/<doc>/images/<stem>.<ext>` references only
 *   - style: a fixed property whitelist, value denylist and character set
 *   - dangerous hosts (script, iframe, svg, …) are dropped with their subtree;
 *     any other non-whitelisted element is unwrapped (children preserved)
 *   - `<img>` without an accepted managed src is dropped entirely
 */

/** The four choke points, mirroring `SanitizeContext` in Rust. */
export type SanitizeContext =
  | "paste_import"
  | "core_to_editor"
  | "editor_to_core"
  | "preview_render";

const MAX_INPUT_BYTES = 4 * 1024 * 1024;
const MAX_URL_LEN = 2048;
const MAX_TEXT_ATTR_LEN = 512;
const MAX_MANAGED_SRC_DEPTH = 4;

const ALLOWED_TAGS: ReadonlySet<string> = new Set([
  "p", "h1", "h2", "h3", "h4", "h5", "h6",
  "strong", "em", "u", "s", "span",
  "ul", "ol", "li", "blockquote",
  "a", "code", "pre", "img", "br",
]);

/** Legacy/authoring spellings folded onto the whitelist (Rust `TAG_ALIASES`). */
const TAG_ALIASES: Readonly<Record<string, string>> = {
  b: "strong",
  i: "em",
  strike: "s",
  del: "s",
  ins: "u",
  tt: "code",
  big: "span",
  small: "span",
  center: "p",
  font: "span",
};

/** Elements removed with their whole subtree (Rust `DROPPED_SUBTREE_TAGS`). */
const DROPPED_SUBTREE_TAGS: ReadonlySet<string> = new Set([
  "script", "style", "iframe", "object", "embed", "applet", "frame",
  "frameset", "noframes", "noembed", "noscript", "template", "xmp",
  "plaintext", "listing", "svg", "math", "base", "meta", "link", "title",
  "form", "input", "button", "select", "textarea", "option", "optgroup",
  "fieldset", "output", "audio", "video", "source", "track", "canvas",
  "dialog", "slot", "portal", "marquee", "blink", "keygen", "param", "isindex",
]);

/** CSS properties accepted inside a `style` attribute. */
const ALLOWED_STYLE_PROPERTIES: ReadonlySet<string> = new Set([
  "background-color", "color", "font-size", "font-style", "font-weight",
  "height", "letter-spacing", "line-height", "text-align", "text-decoration",
  "text-decoration-line", "vertical-align", "width",
]);

const ALLOWED_IMAGE_EXTENSIONS: ReadonlySet<string> = new Set([
  "png", "jpg", "jpeg", "gif", "webp", "bmp",
]);

/** Substrings that must never appear in a CSS value. */
const STYLE_VALUE_DENYLIST: readonly string[] = [
  "url(", "url (", "expression", "javascript", "vbscript", "behavior",
  "-moz-binding", "@import", "/*", "*/",
];

function allowedAttributes(tag: string): readonly string[] {
  switch (tag) {
    case "a": return ["class", "href", "style"];
    case "img": return ["alt", "class", "height", "src", "style", "width"];
    case "ol": return ["class", "start", "style"];
    default: return ["class", "style"];
  }
}

/** Maps a source tag onto the emitted whitelist name; `null` = unwrap. */
export function canonicalTag(tag: string): string | null {
  const lower = tag.toLowerCase();
  if (ALLOWED_TAGS.has(lower)) return lower;
  return TAG_ALIASES[lower] ?? null;
}

/** `class`: safe tokens only, order preserved, deduped, max 8 × 64 chars. */
export function sanitizeClass(raw: string): string | null {
  const tokens: string[] = [];
  for (const token of raw.split(/\s+/)) {
    if (!token || token.length > 64) continue;
    if (!/^[A-Za-z0-9_-]+$/.test(token)) continue;
    if (!tokens.includes(token)) tokens.push(token);
    if (tokens.length === 8) break;
  }
  return tokens.length > 0 ? tokens.join(" ") : null;
}

/** Free-text attribute (`alt`): strip controls, cap the length. */
export function sanitizeTextAttr(raw: string): string {
  let cleaned = "";
  for (const ch of raw.replace(/[\u0000-\u001f\u007f]/g, "")) {
    if (cleaned.length >= MAX_TEXT_ATTR_LEN) break;
    cleaned += ch;
  }
  return cleaned;
}

/** Numeric dimension (`width`, `height`, `start`): `123` or `123px` only. */
export function sanitizeDimension(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed.length === 0 || trimmed.length > 8) return null;
  const digits = trimmed.endsWith("px") ? trimmed.slice(0, -2) : trimmed;
  if (digits.length === 0 || digits.length > 6 || !/^[0-9]+$/.test(digits)) return null;
  return digits;
}

/**
 * Mini CSS sanitizer for the `style` attribute.
 * Returns a canonical, alphabetically sorted `prop: value; …` string, or
 * `null` when the attribute must be refused as a whole.
 */
export function sanitizeStyle(raw: string): string | null {
  if (raw.length > 1024) return null;
  const kept: Array<[string, string]> = [];

  for (const declaration of raw.split(";")) {
    const trimmed = declaration.trim();
    if (trimmed.length === 0) continue;
    const sep = trimmed.indexOf(":");
    if (sep < 0) return null; // malformed style attribute: refuse the whole thing
    const property = trimmed.slice(0, sep).trim().toLowerCase();
    const value = trimmed.slice(sep + 1).trim();
    if (value.length === 0 || value.length > 64) return null;
    if (!ALLOWED_STYLE_PROPERTIES.has(property)) continue;
    if (!isSafeStyleValue(value)) return null;
    if (kept.length >= 16) return null;
    kept.push([property, value]);
  }

  if (kept.length === 0) return null;
  kept.sort((a, b) => a[0].localeCompare(b[0]));

  const seen = new Set<string>();
  const parts: string[] = [];
  for (const [property, value] of kept) {
    if (seen.has(property)) continue;
    seen.add(property);
    parts.push(`${property}: ${value}`);
  }
  const joined = parts.join("; ");
  return joined.length > MAX_TEXT_ATTR_LEN ? null : joined;
}

function isSafeStyleValue(value: string): boolean {
  const lower = value.toLowerCase();
  if (STYLE_VALUE_DENYLIST.some((bad) => lower.includes(bad))) return false;
  return /^[A-Za-z0-9 #%.,\-_()/+]+$/.test(value);
}

function isUrlStrippable(ch: string): boolean {
  const code = ch.codePointAt(0) ?? 0;
  return code <= 0x20 || code === 0x7f;
}

/** Browser-style URL normalization: trim C0-or-space ends, drop tab/LF/CR. */
function normalizeUrlChars(raw: string): string {
  let start = 0;
  let end = raw.length;
  while (start < end && isUrlStrippable(raw[start])) start += 1;
  while (end > start && isUrlStrippable(raw[end - 1])) end -= 1;
  return raw.slice(start, end).replace(/[\t\n\r]/g, "");
}

function isValidSchemeSyntax(scheme: string): boolean {
  return /^[A-Za-z][A-Za-z0-9+\-.]*$/.test(scheme);
}

/**
 * Hyperlink URLs: only `http`/`https` survive. Returns the cleaned value
 * (never the raw input), or `null` when the URL is refused.
 */
export function sanitizeUrl(raw: string): string | null {
  if (raw.length > MAX_URL_LEN) return null;
  const cleaned = normalizeUrlChars(raw);
  if (cleaned.length === 0) return null;
  if (cleaned.includes("\\")) return null;
  for (const ch of cleaned) {
    const code = ch.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) return null;
  }

  const boundary = cleaned.search(/[:/?#]/);
  if (boundary < 0 || cleaned[boundary] !== ":") return null; // scheme required
  const scheme = cleaned.slice(0, boundary);
  if (!isValidSchemeSyntax(scheme)) return null;
  const lower = scheme.toLowerCase();
  if (lower !== "http" && lower !== "https") return null;
  if (!cleaned.slice(boundary + 1).startsWith("//")) return null;
  return cleaned;
}

function isSafePathSegment(segment: string): boolean {
  return segment.length > 0
    && segment.length <= 128
    && segment !== "."
    && segment !== ".."
    && /^[A-Za-z0-9\-_.]+$/.test(segment)
    && !segment.includes("..");
}

/** `<stem>.<ext>` with a safe stem and an allowed lower-case image extension. */
export function validateAssetFileName(name: string): boolean {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return false;
  const stem = name.slice(0, dot);
  const ext = name.slice(dot + 1);
  if (stem.length === 0 || stem.length > 100) return false;
  if (!/^[A-Za-z0-9_-]+$/.test(stem)) return false;
  return ALLOWED_IMAGE_EXTENSIONS.has(ext);
}

/**
 * Validates a document-relative managed image reference of the shape
 * `(../){0,4}.assets/<doc-id>/images/<stem>.<ext>`. Returns the cleaned value
 * or `null` when refused. `expectedDocumentId` pins the reference to the
 * document being edited when provided.
 */
export function sanitizeImageSrc(raw: string, expectedDocumentId?: string | null): string | null {
  if (raw.length > MAX_URL_LEN) return null;
  const cleaned = raw.replace(/^[\s\u0000-\u001f\u007f]+|[\s\u0000-\u001f\u007f]+$/g, "");
  if (cleaned.length === 0) return null;
  if (/[\s\u0000-\u001f\u007f]/.test(cleaned)) return null;
  if (cleaned.includes("%")) return null;
  if (cleaned.includes("\\")) return null;
  if (cleaned.startsWith("//")) return null;
  if (cleaned.startsWith("/")) return null;
  const colon = cleaned.indexOf(":");
  if (colon >= 0 && isValidSchemeSyntax(cleaned.slice(0, colon))) return null;

  let rest = cleaned;
  let depth = 0;
  for (;;) {
    if (rest.startsWith("../")) {
      depth += 1;
      if (depth > MAX_MANAGED_SRC_DEPTH) return null;
      rest = rest.slice(3);
    } else if (rest.startsWith("./")) {
      rest = rest.slice(2);
    } else {
      break;
    }
  }

  const parts = rest.split("/");
  if (parts.length !== 4 || parts[0] !== ".assets" || parts[2] !== "images") return null;
  if (!isSafePathSegment(parts[1])) return null;
  if (expectedDocumentId && parts[1] !== expectedDocumentId) return null;
  if (!validateAssetFileName(parts[3])) return null;
  return cleaned;
}

function isVoidTag(tag: string): boolean {
  return tag === "br" || tag === "img";
}

export interface SanitizeOptions {
  context: SanitizeContext;
  /** Pins managed `img src` references to this document id. */
  documentId?: string | null;
}

/**
 * Runs the whitelist policy over an HTML fragment and returns the safe,
 * re-serialized result. Refusing to render (empty string) is the safe failure
 * mode for oversized input, mirroring the backend's behaviour.
 */
export function sanitizeWith(html: string, options: SanitizeOptions): string {
  if (new TextEncoder().encode(html).length > MAX_INPUT_BYTES) return "";

  const parsed = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const out = parsed.createElement("div");
  appendSanitized(parsed, parsed.body, out, options);
  return out.innerHTML;
}

function appendSanitized(
  doc: Document,
  source: Element,
  target: Element,
  options: SanitizeOptions,
): void {
  source.childNodes.forEach((node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      target.appendChild(doc.createTextNode(node.textContent ?? ""));
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return; // comments/PIs are dropped
    const element = node as Element;
    const rawName = element.localName.toLowerCase();

    if (DROPPED_SUBTREE_TAGS.has(rawName)) return; // subtree dropped
    const canonical = canonicalTag(rawName);
    if (canonical === null) {
      // Not whitelisted: unwrap, keeping the children.
      appendSanitized(doc, element, target, options);
      return;
    }

    const attrs = collectAttributes(element, canonical, options);
    if (canonical === "img" && !attrs.has("src")) return; // unusable image dropped

    const created = doc.createElement(canonical);
    for (const name of Array.from(attrs.keys()).sort()) {
      created.setAttribute(name, attrs.get(name) as string);
    }
    if (!isVoidTag(canonical)) {
      appendSanitized(doc, element, created, options);
    }
    target.appendChild(created);
  });
}

function collectAttributes(
  element: Element,
  tag: string,
  options: SanitizeOptions,
): Map<string, string> {
  const allowed = allowedAttributes(tag);
  const out = new Map<string, string>();

  for (let i = 0; i < element.attributes.length; i += 1) {
    const attr = element.attributes[i];
    const name = attr.name.toLowerCase();
    if (name.startsWith("on") || name.includes(":")) continue; // handlers, namespaced
    if (!allowed.includes(name)) continue;
    const value = attr.value;

    switch (name) {
      case "style": {
        const style = sanitizeStyle(value);
        if (style !== null) out.set("style", style);
        break;
      }
      case "class": {
        const cls = sanitizeClass(value);
        if (cls !== null) out.set("class", cls);
        break;
      }
      case "href": {
        if (tag !== "a") break;
        const href = sanitizeUrl(value);
        if (href !== null) out.set("href", href);
        break;
      }
      case "src": {
        if (tag !== "img") break;
        const src = sanitizeImageSrc(value, options.documentId ?? null);
        if (src !== null) out.set("src", src);
        break;
      }
      case "alt": {
        if (tag !== "img") break;
        out.set("alt", sanitizeTextAttr(value));
        break;
      }
      case "width":
      case "height":
      case "start": {
        const dimension = sanitizeDimension(value);
        if (dimension !== null) out.set(name, dimension);
        break;
      }
      default:
        break;
    }
  }
  return out;
}

// -- The four choke points (RD-M7-013) ----------------------------------------

/** Clipboard / drag-drop HTML entering the application. */
export function sanitizeForPasteImport(html: string, documentId?: string | null): string {
  return sanitizeWith(html, { context: "paste_import", documentId });
}

/** Stored content being fed Core → Editor (never trusted just because we wrote it). */
export function sanitizeForEditor(html: string, documentId?: string | null): string {
  return sanitizeWith(html, { context: "core_to_editor", documentId });
}

/** Editor output being committed Editor → Core (persisted into the XML). */
export function sanitizeForCore(html: string, documentId?: string | null): string {
  return sanitizeWith(html, { context: "editor_to_core", documentId });
}

/** Content being rendered in the read-only preview. */
export function sanitizeForPreview(html: string, documentId?: string | null): string {
  return sanitizeWith(html, { context: "preview_render", documentId });
}
