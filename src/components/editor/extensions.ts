/**
 * Tiptap extension set for the M7 rich-text editor (RD-M7-001~010).
 *
 * Every registered extension produces markup that survives the backend
 * whitelist sanitizer (`src-tauri/src/domain/sanitizer.rs`):
 *   - StarterKit: p, h1-h3, strong/bold, em/italic, s/strike, ul/ol/li,
 *     blockquote, code, hard break (br). `codeBlock` is replaced by
 *     CodeBlockLowlight (pre + code with a whitelisted `class`), `link` and
 *     `underline` are registered standalone below, and `horizontalRule` is
 *     disabled because `<hr>` is not on the whitelist.
 *   - Underline → `<u>`, Strike → `<s>`: whitelisted tags.
 *   - Link → `<a href>` with http/https only; no rel/target attributes are
 *     rendered because the sanitizer would strip them.
 *   - TextStyle + Color + FontSize → `<span style="color|font-size: …">`:
 *     span is whitelisted and both CSS properties are in
 *     `ALLOWED_STYLE_PROPERTIES`.
 *   - TextAlign → `style="text-align: …"` on p/h: whitelisted property.
 *     Registered for paste tolerance only — the approved toolbar has no
 *     alignment control.
 *   - Highlight → rendered as `<span style="background-color: …">` instead of
 *     `<mark>` (which the sanitizer would unwrap). Registered for paste
 *     tolerance only — the approved toolbar has no highlight control.
 *   - Image → `<img src>` limited to relative managed `.assets/…` references
 *     (base64 disabled); inserting images is the M7 asset pipeline, so the
 *     toolbar offers no image control.
 */
import { Extension, mergeAttributes } from "@tiptap/core";
import type { Extensions } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Underline from "@tiptap/extension-underline";
import Link from "@tiptap/extension-link";
import Image from "@tiptap/extension-image";
import TextAlign from "@tiptap/extension-text-align";
import { Color } from "@tiptap/extension-color";
import { FontSize, TextStyle } from "@tiptap/extension-text-style";
import Highlight from "@tiptap/extension-highlight";
import CodeBlockLowlight from "@tiptap/extension-code-block-lowlight";
import { common, createLowlight } from "lowlight";

const lowlight = createLowlight(common);

/**
 * Highlight rendered as a plain `<span style="background-color: …">` so the
 * commit/preview sanitizers keep it. `<mark>` is not on the backend whitelist
 * and would be unwrapped, silently losing the user's highlight.
 */
const SafeHighlight = Highlight.extend({
  addOptions() {
    return {
      ...this.parent?.(),
      multicolor: true,
      HTMLAttributes: {},
    };
  },

  parseHTML() {
    return [
      { tag: "mark" },
      {
        tag: "span",
        getAttrs: (node) => {
          const element = node as HTMLElement;
          const style = typeof element.getAttribute === "function"
            ? element.getAttribute("style") ?? ""
            : "";
          return /background-color\s*:/.test(style) ? {} : false;
        },
      },
    ];
  },

  renderHTML({ HTMLAttributes }) {
    const style = typeof HTMLAttributes.style === "string" ? HTMLAttributes.style : "";
    return ["span", mergeAttributes(this.options.HTMLAttributes, style.length > 0 ? { style } : {}), 0];
  },
});

/**
 * Explicit keyboard shortcuts mapped to editor commands (RD-M7-009). Marks and
 * lists additionally keep Tiptap's built-in bindings (Mod-b, Mod-i,
 * Mod-Shift-7/8, Mod-Shift-b, Mod-z / Mod-Shift-z for the local history).
 */
export function createEditorShortcuts(requestLink: () => void): Extension {
  return Extension.create({
    name: "m7EditorShortcuts",
    addKeyboardShortcuts() {
      return {
        "Mod-u": () => this.editor.commands.toggleUnderline(),
        "Mod-Shift-s": () => this.editor.commands.toggleStrike(),
        "Mod-e": () => this.editor.commands.toggleCode(),
        "Mod-Shift-c": () => this.editor.commands.toggleCodeBlock(),
        "Mod-Alt-1": () => this.editor.commands.toggleHeading({ level: 1 }),
        "Mod-Alt-2": () => this.editor.commands.toggleHeading({ level: 2 }),
        "Mod-Alt-3": () => this.editor.commands.toggleHeading({ level: 3 }),
        "Mod-Alt-0": () => this.editor.commands.setParagraph(),
        "Mod-k": () => {
          requestLink();
          return true;
        },
      };
    },
  });
}

export function buildEditorExtensions(requestLink: () => void): Extensions {
  return [
    StarterKit.configure({
      heading: { levels: [1, 2, 3] },
      link: false,
      underline: false,
      codeBlock: false,
      horizontalRule: false,
    }),
    Underline,
    Link.configure({
      openOnClick: false,
      enableClickSelection: false,
      autolink: true,
      defaultProtocol: "https",
      // The sanitizer only keeps `href`, so never render rel/target noise.
      HTMLAttributes: {},
    }),
    Image.configure({
      inline: false,
      allowBase64: false,
      HTMLAttributes: {},
    }),
    TextAlign.configure({
      types: ["heading", "paragraph"],
    }),
    TextStyle,
    FontSize,
    Color,
    SafeHighlight,
    CodeBlockLowlight.configure({ lowlight }),
    createEditorShortcuts(requestLink),
  ];
}

/** The limited font-size set offered by the toolbar (RD-M7-003). */
export const FONT_SIZES: readonly string[] = ["12px", "14px", "16px", "18px", "24px", "32px"];

export interface TextColorSwatch {
  label: string;
  /** Literal hex so stored HTML stays portable; values mirror the CSS tokens. */
  value: string;
}

/**
 * Content palette for the Text Color control. These are document colors stored
 * inside the XML (legacy TDL cannot resolve our CSS variables), so they are
 * literals; each one mirrors a design token from `src/styles/tokens.css`.
 */
export const TEXT_COLORS: readonly TextColorSwatch[] = [
  { label: "Red", value: "#e53e3e" },      // --color-error
  { label: "Orange", value: "#ed8936" },   // --priority-medium
  { label: "Green", value: "#38a169" },    // --color-success
  { label: "Blue", value: "#3182ce" },     // --color-info
  { label: "Indigo", value: "#5a67d8" },   // --color-accent
  { label: "Gray", value: "#718096" },     // --color-text-muted
  { label: "Ink", value: "#1a1a2e" },      // --color-text-primary
];
