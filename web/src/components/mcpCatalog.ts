// P140 — the gallery of ready-made MCP integrations in the hub's machine settings. It works on the draft of the MCP list (nothing reaches the
// hub until "Save", which asks first and lists what changes): it says which entries the draft already has and gives the fields of a new one.
// Three kinds are here: the local ones (npx and Docker), the hosted ones that take a key in the `Authorization` header, and the hosted ones
// that sign in with OAuth (Slack, Notion's, Sentry's), which the hub signs in after the server is saved (P131, `McpLogin`). Kept free of the
// draft types so the tests can load it; the section builds the draft from what `catalogServer` returns.

import type { Key } from "../i18n";

/** What the person still has to do after adding it: pick a folder on the hub's computer, paste a secret, paste a key as the `Authorization`
 * header, sign the hub in (after saving), or have Docker running there. */
export type McpNeeds = "folder" | "secret" | "header" | "oauth" | "docker";

interface Common {
  id: string;
  label: Key;
  description: Key;
  needs: McpNeeds;
  /** What tells this entry apart in a draft: the npx package or Docker image among a local one's arguments, a hosted one's address. */
  token: string;
}

export interface McpStdioEntry extends Common {
  kind: "stdio";
  command: string;
  /** One argument per line, as the draft holds them. */
  args: string;
  /** The names of the variables the person fills in: the page ships them empty on purpose. */
  envNames: string[];
}

export interface McpHttpEntry extends Common {
  kind: "http";
  url: string;
  /** The names of the headers the person fills in (the key goes in `Authorization`), empty on purpose. None for one that signs in with OAuth. */
  headerNames: string[];
  /** Signs in with OAuth instead of a header: the draft carries it, and the hub signs in once the server is saved. */
  oauth?: boolean;
}

export type McpCatalogEntry = McpStdioEntry | McpHttpEntry;

export const MCP_NEEDS_TEXT: Record<McpNeeds, Key> = {
  folder: "machine.cat.needs.folder",
  secret: "machine.cat.needs.secret",
  header: "machine.cat.needs.header",
  oauth: "machine.cat.needs.oauth",
  docker: "machine.cat.needs.docker",
};

export const MCP_CATALOG: McpCatalogEntry[] = [
  {
    kind: "stdio",
    id: "filesystem",
    label: "machine.cat.filesystem.label",
    description: "machine.cat.filesystem.description",
    needs: "folder",
    token: "@modelcontextprotocol/server-filesystem",
    command: "npx",
    args: "-y\n@modelcontextprotocol/server-filesystem\n/path/to/allowed/dir",
    envNames: [],
  },
  {
    kind: "stdio",
    id: "google_workspace",
    label: "machine.cat.googleWorkspace.label",
    description: "machine.cat.googleWorkspace.description",
    needs: "secret",
    token: "@aaronsb/google-workspace-mcp",
    command: "npx",
    args: "-y\n@aaronsb/google-workspace-mcp",
    envNames: ["GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET"],
  },
  {
    kind: "stdio",
    id: "notion",
    label: "machine.cat.notion.label",
    description: "machine.cat.notion.description",
    needs: "secret",
    token: "@notionhq/notion-mcp-server",
    command: "npx",
    args: "-y\n@notionhq/notion-mcp-server",
    envNames: ["NOTION_TOKEN"],
  },
  {
    kind: "stdio",
    id: "github",
    label: "machine.cat.github.label",
    description: "machine.cat.github.description",
    needs: "docker",
    token: "ghcr.io/github/github-mcp-server",
    command: "docker",
    args: "run\n-i\n--rm\n-e\nGITHUB_PERSONAL_ACCESS_TOKEN\nghcr.io/github/github-mcp-server",
    envNames: ["GITHUB_PERSONAL_ACCESS_TOKEN"],
  },
  // The two below were checked against each vendor's own docs on 2026-10-25 (Session 304) and not against the live services.
  {
    // https://linear.app/docs/mcp: Streamable HTTP at /mcp; an API key goes in `Authorization: Bearer <key>` instead of the OAuth flow.
    kind: "http",
    id: "linear",
    label: "machine.cat.linear.label",
    description: "machine.cat.linear.description",
    needs: "header",
    token: "https://mcp.linear.app/mcp",
    url: "https://mcp.linear.app/mcp",
    headerNames: ["Authorization"],
  },
  {
    // https://docs.github.com/en/copilot/how-tos/provide-context/use-mcp-in-your-ide/set-up-the-github-mcp-server: a personal access token
    // goes in `Authorization: Bearer <token>`.
    kind: "http",
    id: "github_remote",
    label: "machine.cat.githubRemote.label",
    description: "machine.cat.githubRemote.description",
    needs: "header",
    token: "https://api.githubcopilot.com/mcp/",
    url: "https://api.githubcopilot.com/mcp/",
    headerNames: ["Authorization"],
  },
  // The four below sign in with OAuth: Dynamic Client Registration and a browser consent, which the hub runs in two calls (P131). Slack's
  // address was verified on 2026-08-31 (Session 36); Notion's and Sentry's were read in the vendors' docs (Sessions 304 and 313), not tried.
  {
    kind: "http",
    id: "slack",
    label: "machine.cat.slack.label",
    description: "machine.cat.slack.description",
    needs: "oauth",
    token: "https://mcp.slack.com/mcp",
    url: "https://mcp.slack.com/mcp",
    headerNames: [],
    oauth: true,
  },
  {
    kind: "http",
    id: "notion_remote",
    label: "machine.cat.notionRemote.label",
    description: "machine.cat.notionRemote.description",
    needs: "oauth",
    token: "https://mcp.notion.com/mcp",
    url: "https://mcp.notion.com/mcp",
    headerNames: [],
    oauth: true,
  },
  {
    kind: "http",
    id: "sentry",
    label: "machine.cat.sentry.label",
    description: "machine.cat.sentry.description",
    needs: "oauth",
    token: "https://mcp.sentry.dev/mcp",
    url: "https://mcp.sentry.dev/mcp",
    headerNames: [],
    oauth: true,
  },
];

/** The part of a draft that tells which entry it is: how it is told apart depends on its kind. */
interface DraftLike {
  kind: string;
  args: string;
  url: string;
}

/** Whether the draft already has a server of this entry, whatever the person named it: a local one has its package or image as a whole line
 * of the arguments, a hosted one has its address. */
export function catalogState(entry: McpCatalogEntry, existing: DraftLike[]): "added" | "available" {
  const has = existing.some((s) =>
    entry.kind === "stdio" ? s.kind === "stdio" && s.args.split("\n").some((line) => line.trim() === entry.token) : s.kind === "http" && s.url.trim() === entry.token,
  );
  return has ? "added" : "available";
}

export type CatalogServer =
  | { name: string; kind: "stdio"; command: string; args: string; envNames: string[] }
  | { name: string; kind: "http"; url: string; headerNames: string[]; oauth: boolean };

/** The fields of the entry as a new server, named so that it collides with nothing already in the list (`notion`, `notion-2`, `notion-3`). */
export function catalogServer(entry: McpCatalogEntry, existing: { name: string }[]): CatalogServer {
  const taken = new Set(existing.map((s) => s.name.trim()));
  let name = entry.id;
  let n = 2;
  while (taken.has(name)) {
    name = `${entry.id}-${n}`;
    n += 1;
  }
  return entry.kind === "stdio"
    ? { name, kind: "stdio", command: entry.command, args: entry.args, envNames: [...entry.envNames] }
    : { name, kind: "http", url: entry.url, headerNames: [...entry.headerNames], oauth: entry.oauth === true };
}
