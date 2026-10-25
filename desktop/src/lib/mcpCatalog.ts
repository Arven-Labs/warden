// P140 — the gallery of ready-made MCP integrations in Settings. The list is a draft in the form until "Save", so the gallery works on
// that draft: it says which entries the draft already has and puts a new server in it, and nothing else. The first five are the ones
// the quick-add buttons always offered (Session 35 and 36); the hosted ones after them were added only after their address, transport and
// sign-in were read in the vendor's own docs. No address that was not checked is added here.

import type { Key } from "../i18n/index.ts";
import { isMcpServerHttp, type McpServer } from "../types.ts";

/** What the person still has to do after adding it: pick a folder, paste a secret, paste a key as the `Authorization` header, sign in in the
 * browser, or have Docker running. */
export type McpNeeds = "folder" | "secret" | "header" | "oauth" | "docker";

export interface McpCatalogEntry {
  id: string;
  /** The gallery's title for it. */
  label: Key;
  description: Key;
  needs: McpNeeds;
  /** What tells this entry apart in a server's `args` (the npx package, the Docker image). Unused for a hosted one, which `url` tells apart. */
  token: string;
  /** What adding writes, with the secrets left blank on purpose: the person fills in their own. */
  server: McpServer;
}

export const MCP_NEEDS_TEXT: Record<McpNeeds, Key> = {
  folder: "st.mcpCat.needs.folder",
  secret: "st.mcpCat.needs.secret",
  header: "st.mcpCat.needs.header",
  oauth: "st.mcpCat.needs.oauth",
  docker: "st.mcpCat.needs.docker",
};

export const MCP_CATALOG: McpCatalogEntry[] = [
  {
    id: "filesystem",
    label: "st.mcpCat.filesystem.label",
    description: "st.mcpCat.filesystem.description",
    needs: "folder",
    token: "@modelcontextprotocol/server-filesystem",
    server: { name: "filesystem", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/dir"], env: {} },
  },
  {
    id: "google_workspace",
    label: "st.mcpCat.googleWorkspace.label",
    description: "st.mcpCat.googleWorkspace.description",
    needs: "secret",
    token: "@aaronsb/google-workspace-mcp",
    server: { name: "google_workspace", command: "npx", args: ["-y", "@aaronsb/google-workspace-mcp"], env: { GOOGLE_CLIENT_ID: "", GOOGLE_CLIENT_SECRET: "" } },
  },
  {
    id: "notion",
    label: "st.mcpCat.notion.label",
    description: "st.mcpCat.notion.description",
    needs: "secret",
    token: "@notionhq/notion-mcp-server",
    server: { name: "notion", command: "npx", args: ["-y", "@notionhq/notion-mcp-server"], env: { NOTION_TOKEN: "" } },
  },
  {
    // See ARCHITECTURE.md for why GitHub goes through Docker.
    id: "github",
    label: "st.mcp.githubDocker",
    description: "st.mcpCat.github.description",
    needs: "docker",
    token: "ghcr.io/github/github-mcp-server",
    server: {
      name: "github",
      command: "docker",
      args: ["run", "-i", "--rm", "-e", "GITHUB_PERSONAL_ACCESS_TOKEN", "ghcr.io/github/github-mcp-server"],
      env: { GITHUB_PERSONAL_ACCESS_TOKEN: "" },
    },
  },
  {
    // Verified endpoint (2026-08-31, Session 36 / P25): Streamable HTTP with real OAuth (discovery, Dynamic Client Registration,
    // browser consent — PENDING.md P26). The card's "Connect" button authorizes it after saving.
    id: "slack",
    label: "st.mcp.slack",
    description: "st.mcpCat.slack.description",
    needs: "oauth",
    token: "https://mcp.slack.com/mcp",
    server: { name: "slack", url: "https://mcp.slack.com/mcp", headers: {}, oauth: true },
  },
  // The three below were checked against each vendor's own docs on 2026-10-25 (Session 304) and not against the live services.
  {
    // https://linear.app/docs/mcp: Streamable HTTP at /mcp; an API key goes in `Authorization: Bearer <key>` instead of the OAuth flow.
    id: "linear",
    label: "st.mcpCat.linear.label",
    description: "st.mcpCat.linear.description",
    needs: "header",
    token: "https://mcp.linear.app/mcp",
    server: { name: "linear", url: "https://mcp.linear.app/mcp", headers: { Authorization: "" }, oauth: false },
  },
  {
    // https://docs.github.com/en/copilot/how-tos/provide-context/use-mcp-in-your-ide/set-up-the-github-mcp-server: a personal access token
    // goes in `Authorization: Bearer <token>`. The local Docker entry above stays: it is the same tools without a hosted dependency.
    id: "github_remote",
    label: "st.mcpCat.githubRemote.label",
    description: "st.mcpCat.githubRemote.description",
    needs: "header",
    token: "https://api.githubcopilot.com/mcp/",
    server: { name: "github_remote", url: "https://api.githubcopilot.com/mcp/", headers: { Authorization: "" }, oauth: false },
  },
  {
    // https://developers.notion.com/guides/mcp/get-started-with-mcp: Streamable HTTP with OAuth (discovery and dynamic registration), the
    // same flow as Slack's. The hosted server documents no token header; the npx entry above is the older, unmaintained one.
    id: "notion_remote",
    label: "st.mcpCat.notionRemote.label",
    description: "st.mcpCat.notionRemote.description",
    needs: "oauth",
    token: "https://mcp.notion.com/mcp",
    server: { name: "notion_remote", url: "https://mcp.notion.com/mcp", headers: {}, oauth: true },
  },
];

/** Whether the draft already has a server of this entry, whatever the person named it: a hosted one by its address, a local one by its
 * package or image among the arguments. */
export function catalogState(entry: McpCatalogEntry, servers: McpServer[]): "added" | "available" {
  const has = servers.some((s) => (isMcpServerHttp(s) ? s.url === entry.token : s.args.includes(entry.token)));
  return has ? "added" : "available";
}

/** The entry as a new server for the draft: a copy (editing its card never touches the catalog) named so that it collides with nothing
 * already there (`notion`, `notion-2`, `notion-3`). */
export function catalogServer(entry: McpCatalogEntry, servers: McpServer[]): McpServer {
  const base: McpServer = isMcpServerHttp(entry.server)
    ? { ...entry.server, headers: { ...entry.server.headers } }
    : { ...entry.server, args: [...entry.server.args], env: { ...entry.server.env } };
  let name = base.name;
  let n = 2;
  while (servers.some((s) => s.name === name)) {
    name = `${base.name}-${n}`;
    n += 1;
  }
  return { ...base, name };
}
