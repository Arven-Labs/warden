// P-49, desktop: what the rotation block does and says when a call fails. Pure, so `npm test` proves the decision (the component
// has no test of its own).

import type { Key } from "../i18n/en.ts";

/** What `arca_rotation_confirm` rejects with. `partial`: the rotation may have written something before it stopped. `newerEpoch`: a newer
 * key epoch is stored; on a resume that suggests the phrase typed may not be the one of the rotation that stopped (a forged object of a
 * newer epoch gives the same error, so it is not proof). `unknown`: the rejection is not an answer of the backend's own (a plain string:
 * a broken channel, a panic), so nobody knows whether something was written. */
export interface RotationFailure {
  partial: boolean;
  message: string;
  newerEpoch: boolean;
  unknown: boolean;
}

/** Where the 24 words are: `shown` (a key just made) or `resume` (typed by the person). */
export type RotationStep = "shown" | "resume";

/** Which call failed: `resume` is `arca_rotation_resume` (it reads the phrase and writes nothing), `confirm` is `arca_rotation_confirm`. */
export type RotationPhase = "resume" | "confirm";

export interface FailureView {
  /** `resume`: go to (or stay in) the resume with the same 24 words. `keep`: stay in the resume, words kept. `reset`: the key was
   * dropped, start again. */
  next: "resume" | "keep" | "reset";
  /** The key of the text in `i18n`; every one of them fills `{message}`. */
  key: Key;
}

/** Any rejection, as a failure. An object with a `message` is the backend's own answer (only a literal `true` counts in its flags); anything
 * else (a plain string) is `unknown`: `arca_rotation_resume` rejects with strings too, and `failureView` tells the two calls apart. */
export function failureOf(e: unknown): RotationFailure {
  if (typeof e === "object" && e !== null && "message" in e) {
    const { partial, message, newerEpoch } = e as { partial?: unknown; message: unknown; newerEpoch?: unknown };
    return { partial: partial === true, message: String(message), newerEpoch: newerEpoch === true, unknown: false };
  }
  return { partial: false, message: String(e), newerEpoch: false, unknown: true };
}

/** What to do and say after `phase` of a rotation at `step` failed.
 *
 * - A stop that may have written something (`partial`), or an `unknown` rejection of the **confirm** (the same rule as the phone: the
 *   unknown may have written something), goes to the resume with the same 24 words.
 * - A key just made (`shown`) that was refused is spent: start again, with the plain text.
 * - On a resume, a refusal of the **confirm** is about THIS attempt: an earlier one may have published the key, so the text does not
 *   say "not changed" and the words stay. A newer epoch there suggests the phrase may not be the published one (it is not proof), and
 *   has its own text.
 * - A failure of the **resume call** itself (not 24 valid words, not in arca mode) is about the phrase, not about any attempt: the
 *   plain text, words kept. Saying "an earlier attempt may have published this key" about a phrase that is not even valid would be false. */
export function failureView(step: RotationStep, phase: RotationPhase, failure: RotationFailure): FailureView {
  if (failure.partial || (phase === "confirm" && failure.unknown)) return { next: "resume", key: "arca.rotationPartial" };
  if (step === "shown") return { next: "reset", key: "arca.rotationFailed" };
  if (phase === "resume") return { next: "keep", key: "arca.rotationFailed" };
  return { next: "keep", key: failure.newerEpoch ? "arca.rotationResumeNewerEpoch" : "arca.rotationResumeNotWritten" };
}
