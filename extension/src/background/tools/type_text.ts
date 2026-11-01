import type { ToolSpec } from "../../protocol/messages";
import { parseTypeTextArgs, refusalFor, type FieldFacts } from "../../protocol/domGuards";
import { t } from "../../i18n/index.ts";
import { localApprovals } from "../local_approval";
import { runInPage } from "./dom_executor";

export const typeTextSpec: ToolSpec = {
  name: "browser_type_text",
  description:
    "Types text into one text field of a browser tab, identified by a CSS selector (as returned by browser_read_page). It never presses Enter " +
    "or submits a form: to send it, call browser_click_element on the button afterwards. It refuses password fields, one-time codes and card " +
    "fields (the person types those), and it first asks the person for a yes in the side panel; no answer in a minute counts as a no. " +
    "'text' replaces what the field holds unless 'append' is true; an empty text clears the field.",
  parameters: {
    type: "object",
    properties: {
      selector: { type: "string", description: "CSS selector of the text field" },
      text: { type: "string", description: "The text to type (up to 2000 characters). Empty clears the field." },
      append: { type: "boolean", description: "Add to what the field already holds instead of replacing it. Default false." },
      tabId: { type: "number", description: "Tab id from browser_list_tabs to act on — omit to use whichever tab is currently active/focused." },
    },
    required: ["selector", "text"],
  },
};

/** How long the person has to answer in the side panel before it counts as a no. */
const APPROVAL_TIMEOUT_MS = 60_000;
const PREVIEW_CHARS = 200;

/** What the page says about the field, plus where it is. Compared as a whole between the two phases. */
type Snapshot = FieldFacts & { origin: string };

interface TypeTextResult {
  typed: true;
  tag: string;
  /** How many characters the field holds now. */
  length: number;
}

/**
 * Runs inside the page — must be fully self-contained, see `dom_executor.ts`. One function for both phases so the facts are gathered by the
 * same code: `inspect` only reports them; `write` gathers them again, refuses if anything differs from what was inspected (and approved), and
 * only then writes. Between the two the person was asked, and a page can swap a field for another in that time.
 */
function fieldInPage(mode: "inspect" | "write", selector: string, text: string, append: boolean, expected: Snapshot | null): Snapshot | TypeTextResult {
  const element = document.querySelector(selector);
  if (!element) {
    throw new Error(`no element matches selector '${selector}'`);
  }
  const html = element as HTMLInputElement & HTMLTextAreaElement;
  const rect = element.getBoundingClientRect();
  const style = getComputedStyle(element);
  const label = (html.labels?.[0]?.innerText || element.getAttribute("aria-label") || element.getAttribute("placeholder") || "").trim().slice(0, 200);
  const snapshot: Snapshot = {
    tag: element.tagName.toLowerCase(),
    type: element.tagName === "INPUT" ? (element.getAttribute("type") ?? "") : "",
    autocomplete: element.getAttribute("autocomplete") ?? "",
    name: (element.getAttribute("name") ?? "").slice(0, 200),
    id: (element.getAttribute("id") ?? "").slice(0, 200),
    label,
    readOnly: html.readOnly === true,
    disabled: html.disabled === true,
    visible: rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none",
    origin: location.origin,
  };
  if (mode === "inspect") return snapshot;

  if (JSON.stringify(snapshot) !== JSON.stringify(expected)) {
    throw new Error("the field changed after it was checked and approved; nothing was typed");
  }
  const proto = element.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  // The setter of the prototype, not `element.value = …`: pages built on React and the like keep their own value and ignore a plain assignment.
  const setValue = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  if (!setValue) {
    throw new Error("this field cannot be written to");
  }
  html.focus();
  setValue.call(html, append ? html.value + text : text);
  html.dispatchEvent(new Event("input", { bubbles: true }));
  html.dispatchEvent(new Event("change", { bubbles: true }));
  return { typed: true, tag: snapshot.tag, length: html.value.length };
}

/** A line the person can read in the approval card: the text about to be typed, shortened, with control characters made visible. */
function previewOf(text: string): string {
  const flat = text.replace(/\r?\n/g, "↵").replace(/\t/g, "→");
  return flat.length > PREVIEW_CHARS ? `${flat.slice(0, PREVIEW_CHARS)}…` : flat;
}

export async function typeText(args: unknown): Promise<TypeTextResult> {
  const { selector, text, append, tabId } = parseTypeTextArgs(args);

  // 1. Look at the field without touching it.
  const facts = (await runInPage(fieldInPage, ["inspect", selector, "", false, null], tabId)) as Snapshot;
  const refusal = refusalFor(facts);
  if (refusal !== null) {
    throw new Error(`browser_type_text refused: ${refusal}`);
  }

  // 2. Ask the person, in the side panel (the icon shows a "!" with the panel closed). Fails closed.
  const field = facts.label || facts.name || facts.id || facts.tag;
  const yes = await localApprovals.request(
    {
      action: "browser_type_text",
      target: `${selector} @ ${facts.origin}`,
      detail: t(append ? "bg.typeDetailAppend" : "bg.typeDetailReplace", { field, length: String(text.length), preview: previewOf(text) }),
    },
    APPROVAL_TIMEOUT_MS,
  );
  if (!yes) {
    throw new Error("the person did not approve typing into this field (no answer within a minute counts as a no); nothing was typed");
  }

  // 3. Write, if the field is still exactly the one that was checked and approved.
  return (await runInPage(fieldInPage, ["write", selector, text, append, facts], tabId)) as TypeTextResult;
}
