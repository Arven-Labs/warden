import { readFileSync } from "node:fs";
import { join } from "node:path";

/** Doc pages in sidebar order; each slug is a file in `content/docs/`. */
export const DOC_SLUGS = [
  "overview",
  "desktop",
  "hub",
  "daily",
  "api",
  "tasks",
  "nodes",
  "people",
  "backup",
  "troubleshooting",
  "status",
] as const;

export type DocSlug = (typeof DOC_SLUGS)[number];

export interface Doc {
  slug: string;
  title: string;
  body: string;
}

const CONTENT_DIR = join(process.cwd(), "content", "docs");

/** Loads one page; the title is its first `# ` heading. */
export function loadDoc(slug: string): Doc {
  const raw = readFileSync(join(CONTENT_DIR, `${slug}.md`), "utf8");
  const match = /^#\s+(.+)$/m.exec(raw);
  if (!match) throw new Error(`docs: ${slug}.md has no "# " title`);
  return { slug, title: match[1].trim(), body: raw };
}

export function allDocs(): Doc[] {
  return DOC_SLUGS.map(loadDoc);
}
