// Signing the hub in to an OAuth MCP server from the desktop (P131), the part that does not need a DOM. The desktop listens for the browser
// itself, so what the hub is handed is the address the browser arrived at, put back together from the query that reached the listener.

/** Why the query that reached the listener cannot finish the sign-in; the words for each are `st.hubLogin.problem.<problem>`. */
export type ReturnProblem = "denied" | "noCode";

/** The address to hand to the hub for the query `code=…&state=…` that reached the listener, or why it will not do. `redirectPrefix` is
 * what the hub said the return address starts with. A browser that was refused (`error=` in the query) says so. */
export function callbackAddress(redirectPrefix: string, query: string): { address: string } | { problem: ReturnProblem } {
  const fields = new URLSearchParams(query.replace(/^\?/, ""));
  if (fields.has("error")) return { problem: "denied" };
  if (!fields.get("code") || !fields.get("state")) return { problem: "noCode" };
  return { address: `${redirectPrefix}?${query.replace(/^\?/, "")}` };
}
