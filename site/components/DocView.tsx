import Link from "next/link";
import type { ComponentProps } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { allDocs } from "../lib/docs.ts";
import { Logo } from "./Logo.tsx";
import { ThemeToggle } from "./ThemeToggle.tsx";

/** Internal links go through `Link` so they pick up the `basePath`; external ones stay plain. */
function MarkdownLink({ href, children }: ComponentProps<"a">) {
  if (href?.startsWith("/")) return <Link href={href}>{children}</Link>;
  return <a href={href}>{children}</a>;
}

export function DocView({ slug }: { slug: string }) {
  const docs = allDocs();
  const index = docs.findIndex((d) => d.slug === slug);
  const doc = docs[index];
  const prev = docs[index - 1];
  const next = docs[index + 1];

  return (
    <div className="shell">
      <nav className="sidebar" aria-label="Documentation">
        <div className="sidebar-top">
          <Link href="/" className="brand" aria-label="Warden home">
            <Logo />
          </Link>
          <ThemeToggle />
        </div>
        <ul>
          {docs.map((d) => (
            <li key={d.slug}>
              <Link href={`/docs/${d.slug}/`} aria-current={d.slug === slug ? "page" : undefined}>
                {d.title}
              </Link>
            </li>
          ))}
        </ul>
      </nav>
      <main className="content">
        <article>
          <Markdown remarkPlugins={[remarkGfm]} components={{ a: MarkdownLink }}>
            {doc.body}
          </Markdown>
        </article>
        <footer className="pager">
          {prev ? <Link href={`/docs/${prev.slug}/`}>← {prev.title}</Link> : <span />}
          {next ? <Link href={`/docs/${next.slug}/`}>{next.title} →</Link> : <span />}
        </footer>
      </main>
    </div>
  );
}
