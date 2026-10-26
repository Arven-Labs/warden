// Signing the hub in to an OAuth MCP server from the web (P131), the part that does not need a DOM. The person's browser ends the sign-in on
// an address that does not open (nothing listens there on purpose) and they copy it back; this reads what they pasted and says what is wrong
// with it before a round trip. The hub checks the address again against the flow it opened, so these checks only spare the trip.

/** What is wrong with the pasted text; the words for each are `machine.login.problem.<problem>`. */
export type PasteProblem = "empty" | "notAnAddress" | "wrongPlace" | "denied" | "noCode";

/** The address to hand to the hub, or why it will not do. `redirectPrefix` is what the hub said the address starts with. Spaces and the
 * quotes or angle brackets a copy sometimes brings are taken off; a browser that was refused (`error=` in the address) says so. */
export function checkPastedAddress(text: string, redirectPrefix: string): { address: string } | { problem: PasteProblem } {
  const address = text.trim().replace(/^["'<]+|["'>]+$/g, "").trim();
  if (address === "") return { problem: "empty" };
  if (!/^https?:\/\/\S+$/.test(address)) return { problem: "notAnAddress" };
  if (!address.startsWith(`${redirectPrefix}?`)) return { problem: "wrongPlace" };
  const query = new URLSearchParams(address.slice(redirectPrefix.length + 1));
  if (query.has("error")) return { problem: "denied" };
  if (!query.get("code") || !query.get("state")) return { problem: "noCode" };
  return { address };
}
