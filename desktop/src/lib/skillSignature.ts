import type { PackagePreview } from "../types";

// Who signed a skill package, as the import preview says it (P160). Kept out of the component so the rules are testable without a DOM.

/** `trusted`: signed by a key the person listed in `[[trusted_publishers]]`. `unknown`: signed, by a key they did not list.
 * `unsigned`: nothing says who wrote it. */
export type SignatureKind = "trusted" | "unknown" | "unsigned";

export interface SignatureView {
  kind: SignatureKind;
  /** The name the person gave the key, when it is listed and has one. */
  name?: string;
  /** The first characters of the signing key, enough to tell two keys apart in a line of text. */
  key?: string;
  /** The whole key, for copying it into the list of keys the person trusts. */
  fullKey?: string;
}

const SHORT_KEY_CHARS = 16;

/** What to say about the signature of `preview`. Only the exact word `trusted` is ever shown as trusted: a hub that predates
 * signing sends nothing, and one that is newer than this app may send a state it has not heard of; both read as no claim. */
export function signatureView(preview: Pick<PackagePreview, "signature" | "signer" | "signerName">): SignatureView {
  const key = preview.signer ? preview.signer.slice(0, SHORT_KEY_CHARS) : undefined;
  if (preview.signature === "trusted") return { kind: "trusted", name: preview.signerName || undefined, key, fullKey: preview.signer };
  if (preview.signature === "unknown" && key) return { kind: "unknown", key, fullKey: preview.signer };
  return { kind: "unsigned" };
}
