import type { ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import type { Components, ExtraProps } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { codeLanguage, hastText, type HastNode } from "../lib/clipboard";
import CopyButton from "./CopyButton";

interface ElementNode extends HastNode {
  properties?: { className?: string | string[] };
}

/** A fenced block: its language and a button that copies the code as it was written, above the (highlighted) code. */
function CodeBlock({ node, children }: ExtraProps & { children?: ReactNode }) {
  const code = (node as unknown as ElementNode | undefined)?.children?.[0] as ElementNode | undefined;
  const language = codeLanguage(code?.properties?.className);
  return (
    <div className="code-block">
      <div className="code-block-bar">
        <span className="code-block-lang">{language ?? ""}</span>
        <CopyButton text={() => hastText(code)} />
      </div>
      <pre>{children}</pre>
    </div>
  );
}

/** The markdown of a chat message (P157): tables, links the app opens itself, code highlighted when the fence names a language
 * and a button to copy each block. Only a fence that names its language is coloured; guessing would paint prose by mistake. */
function Markdown({ children, link }: { children: string; link: Components["a"] }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[[rehypeHighlight, { detect: false, ignoreMissing: true }]]}
      components={{ a: link, pre: CodeBlock }}
    >
      {children}
    </ReactMarkdown>
  );
}

export default Markdown;
