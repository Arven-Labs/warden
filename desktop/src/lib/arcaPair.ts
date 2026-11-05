// P182 — the Sync screen's side of the arca pairing: the `arca-pair-event`s of `arca_pair_cmds.rs` folded into one view.
//
// What the device computes (`show`) is drawn as text to read out loud; what the person types is only ever asked for (`ask`),
// and an `ask` carries no value, so there is nothing to pre-fill a field with.

export type PairEvent =
  | { kind: "show"; what: "offerCode" | "keyFingerprint"; value: string }
  | { kind: "ask"; ask: "codeShownByTheNewDevice" | "fingerprintShownByTheOldDevice" }
  | { kind: "choose"; devices: string[] };

export type PairQuestion = "code" | "fingerprint" | "choice";

export interface PairView {
  /** Something this device computed, for the person to read to the other device. */
  shown: { what: "offerCode" | "keyFingerprint"; value: string } | null;
  /** What the person is being asked now, if anything. */
  question: PairQuestion | null;
  /** The waiting devices to choose from, when `question` is `choice`. */
  devices: string[];
}

export const emptyView: PairView = { shown: null, question: null, devices: [] };

/** The view after `event`; an event this screen does not know leaves it as it was. */
export function reduce(view: PairView, event: unknown): PairView {
  const e = event as PairEvent | null;
  if (!e || typeof e !== "object") return view;
  switch (e.kind) {
    case "show":
      return { shown: { what: e.what, value: e.value }, question: null, devices: [] };
    case "ask":
      // The question replaces whatever was shown: nothing computed sits beside the field the person types into.
      return { shown: null, question: e.ask === "codeShownByTheNewDevice" ? "code" : "fingerprint", devices: [] };
    case "choose":
      return { shown: null, question: "choice", devices: e.devices };
    default:
      return view;
  }
}

/** Whether an edit of a code field came from the keyboard. Pasting, dropping and autofill defeat the check (the point is to
 * read one screen and type on the other), so the field refuses them. A help, not a guarantee: a person can still copy from
 * one device and type on the other. */
export function fieldIsTyped(edit: { inputType: string }): boolean {
  return !NOT_TYPED.has(edit.inputType);
}

/** The `InputEvent.inputType`s that put text in a field without the person typing it. */
const NOT_TYPED = new Set(["insertFromPaste", "insertFromPasteAsQuotation", "insertFromDrop", "insertFromYank", "insertReplacementText"]);
