// P140 — the gallery of ready-made MCP integrations in the hub's machine settings. It works on the draft of the MCP list (nothing reaches the
// hub until "Save", which asks first and lists what changes): it says which entries the draft already has and gives the fields of a new one.
// Only the local ones are here (npx and Docker): a hosted one that signs in with OAuth (Slack) needs the desktop, and the page never sends
// `oauth`. Kept free of the draft types so the tests can load it; the section builds the draft from what `catalogServer` returns.

import type { Key } from "../i18n";

/** What the person still has to do after adding it: pick a folder on the hub's computer, paste a secret, or have Docker running there. */
export type McpNeeds = "folder" | "secret" | "docker";

export interface McpCatalogEntry {
  id: string;
  label: Key;
  description: Key;
  needs: McpNeeds;
  /** What tells this entry apart in a server's arguments (the npx package, the Docker image). */
  token: string;
  command: string;
  /** One argument per line, as the draft holds them. */
  args: string;
  /** The names of the variables the person fills in: the page ships them empty on purpose. */
  envNames: string[];
}

export const MCP_NEEDS_TEXT: Record<McpNeeds, Key> = {
  folder: "machine.cat.needs.folder",
  secret: "machine.cat.needs.secret",
  docker: "machine.cat.needs.docker",
};

export const MCP_CATALOG: McpCatalogEntry[] = [
  {
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
    id: "github",
    label: "machine.cat.github.label",
    description: "machine.cat.github.description",
    needs: "docker",
    token: "ghcr.io/github/github-mcp-server",
    command: "docker",
    args: "run\n-i\n--rm\n-e\nGITHUB_PERSONAL_ACCESS_TOKEN\nghcr.io/github/github-mcp-server",
    envNames: ["GITHUB_PERSONAL_ACCESS_TOKEN"],
  },
];

/** Whether the draft already has a server of this entry, whatever the person named it: its package or image is among the arguments. */
export function catalogState(entry: McpCatalogEntry, existing: { kind: string; args: string }[]): "added" | "available" {
  const has = existing.some((s) => s.kind === "stdio" && s.args.split("\n").some((line) => line.trim() === entry.token));
  return has ? "added" : "available";
}

/** The fields of the entry as a new server, named so that it collides with nothing already in the list (`notion`, `notion-2`, `notion-3`). */
export function catalogServer(entry: McpCatalogEntry, existing: { name: string }[]): { name: string; command: string; args: string; envNames: string[] } {
  const taken = new Set(existing.map((s) => s.name.trim()));
  let name = entry.id;
  let n = 2;
  while (taken.has(name)) {
    name = `${entry.id}-${n}`;
    n += 1;
  }
  return { name, command: entry.command, args: entry.args, envNames: [...entry.envNames] };
}
