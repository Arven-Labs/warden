import Link from "next/link";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { allDocs } from "../lib/docs.ts";

export function DocView({ slug }: { slug: string }) {
  const docs = allDocs();
  const index = docs.findIndex((d) => d.slug === slug);
  const doc = docs[index];
  const prev = docs[index - 1];
  const next = docs[index + 1];

  return (
    <div className="shell">
      <nav className="sidebar" aria-label="Documentation">
        <Link href="/" className="brand">
          Warden
        </Link>
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
          <Markdown remarkPlugins={[remarkGfm]}>{doc.body}</Markdown>
        </article>
        <footer className="pager">
          {prev ? <Link href={`/docs/${prev.slug}/`}>← {prev.title}</Link> : <span />}
          {next ? <Link href={`/docs/${next.slug}/`}>{next.title} →</Link> : <span />}
        </footer>
      </main>
    </div>
  );
}
