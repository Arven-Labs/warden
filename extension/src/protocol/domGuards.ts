// The rules of `browser_type_text` that decide, before anything is written, whether a field may be typed into, and how its arguments are read.
// The model chooses both the field and the text, and a page the model has read can tell it what to choose, so these are the lines that keep a
// hostile page from having a password, a card number or a one-time code filled for it. Pure, so it is tested without a browser
// (`tests/domGuards.test.mjs`); `background/tools/type_text.ts` looks at the field and writes into it.
//
// A refusal is about the field, never about the text: the person can always type there themselves. Sending the form is not an argument at all
// (no Enter, no submit): that stays with `browser_click_element`, which the hub already holds as an external message.

/** The most characters one call types: a sentence or a paragraph, not a document. */
export const MAX_TYPED_CHARS = 2_000;

/** What the page reports about a field, gathered without changing it. */
export interface FieldFacts {
  tag: string;
  /** The `type` attribute of an `<input>`; empty for anything else. */
  type: string;
  autocomplete: string;
  name: string;
  id: string;
  /** The text of the field's label, or its `aria-label`, or its placeholder. */
  label: string;
  readOnly: boolean;
  disabled: boolean;
  visible: boolean;
}

/** The kinds of `<input>` that hold plain text. A `type` that is missing or unknown behaves as `text`, so an empty one counts. */
const TEXT_INPUT_TYPES = new Set(["text", "search", "email", "url", "tel", "number", ""]);

/** The `autocomplete` tokens that say a field holds a secret: a password, a one-time code, any card detail (`cc-*`). */
const SECRET_AUTOCOMPLETE = /^(current-password|new-password|one-time-code|cc-.+)$/;

/** Words that name a secret, as whole words: `user_password`, `passwd`, `Senha`, `cvv`, `card-cvc`, `One-time code`, `otp`, `newPassword`. Whole words,
 * so `passport_country` and `compass` are not caught. Some sites mask a password with a plain text input, which is why the name is read at all. */
const SECRET_WORD = /(^|[^a-z])(pass(word|wd)?|senha|cvv|cvc|otp|one[-_ ]?time)([^a-z]|$)/i;

const SECRET_REFUSAL = "this field is for a credential, secret or card detail (password, one-time code, card number); type it yourself";

/** Splits `newPassword` into `new Password`, so the words are found in a camelCase name. */
function splitCamelCase(text: string): string {
  return text.replace(/([a-z])([A-Z])/g, "$1 $2");
}

/** Why `facts` may not be typed into, or `null` when it may. */
export function refusalFor(facts: FieldFacts): string | null {
  const tag = facts.tag.toLowerCase();
  const type = facts.type.trim().toLowerCase();

  if (tag !== "input" && tag !== "textarea") {
    return `the element is a <${tag}>, not a text field`;
  }
  if (tag === "input") {
    if (type === "password") {
      return "this is a password field; the password has to be typed by the person";
    }
    if (!TEXT_INPUT_TYPES.has(type)) {
      return `this input is of type '${type}', not a text field`;
    }
  }

  const tokens = facts.autocomplete.toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.some((token) => SECRET_AUTOCOMPLETE.test(token))) {
    return SECRET_REFUSAL;
  }
  if (SECRET_WORD.test(splitCamelCase([facts.name, facts.id, facts.label].join(" | ")))) {
    return SECRET_REFUSAL;
  }

  if (facts.disabled) return "the field is disabled";
  if (facts.readOnly) return "the field is read-only";
  if (!facts.visible) return "the field is not visible on the page";
  return null;
}

export interface TypeTextArgs {
  selector: string;
  text: string;
  /** Add to what the field has instead of replacing it. */
  append: boolean;
  tabId?: number;
}

export function parseTypeTextArgs(args: unknown): TypeTextArgs {
  if (typeof args !== "object" || args === null || Array.isArray(args)) {
    throw new Error("the arguments of browser_type_text must be an object with a 'selector' and a 'text'");
  }
  const { selector, text, append, tabId } = args as Record<string, unknown>;

  if (typeof selector !== "string" || selector.trim() === "") {
    throw new Error("'selector' must be a non-empty CSS selector");
  }
  if (typeof text !== "string") {
    throw new Error("'text' must be a string (an empty one clears the field)");
  }
  if (text.length > MAX_TYPED_CHARS) {
    throw new Error(`'text' is longer than ${MAX_TYPED_CHARS} characters; type it in parts`);
  }
  if (append !== undefined && typeof append !== "boolean") {
    throw new Error("'append' must be true or false when provided");
  }
  if (tabId !== undefined && typeof tabId !== "number") {
    throw new Error("'tabId' argument must be a number when provided");
  }
  // Only these four are carried through: whatever else the model sends (a `submit`, a `pressEnter`) is dropped, not honoured.
  return { selector, text, append: append === true, ...(tabId !== undefined && { tabId }) };
}
