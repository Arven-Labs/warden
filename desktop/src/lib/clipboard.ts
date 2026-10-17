/** Copying text, and the two small readings the chat's code blocks need to offer a copy (P157). */

/** Copies `text` to the clipboard. True when it worked. The modern API first; where it is missing (a page served over plain
 * `http://` is not a secure context) a hidden `textarea` and `execCommand("copy")`, the old way, which still works there. */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Refused (no permission, not focused): the old way may still work.
  }
  return copyWithTextarea(text);
}

function copyWithTextarea(text: string): boolean {
  if (typeof document === "undefined") return false;
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  try {
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    area.remove();
  }
}

/** The language of a fenced code block from the classes of its `<code>` (`language-ts` is `ts`), or null when it names none. */
export function codeLanguage(className: string | string[] | undefined): string | null {
  const classes = Array.isArray(className) ? className : (className ?? "").split(/\s+/);
  const named = classes.find((c) => c.startsWith("language-") && c.length > "language-".length);
  return named ? named.slice("language-".length) : null;
}

/** The part of a markdown (hast) tree this file reads: text leaves and elements with children. */
export interface HastNode {
  type: string;
  value?: string;
  children?: HastNode[];
}

/** The plain text of a tree, without the `<span>`s the highlighter put around the words: what a code block's copy button copies. */
export function hastText(node: HastNode | undefined): string {
  if (!node) return "";
  if (node.type === "text") return node.value ?? "";
  return (node.children ?? []).map(hastText).join("");
}
