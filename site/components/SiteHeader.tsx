import Link from "next/link";
import { Logo } from "./Logo.tsx";
import { ThemeToggle } from "./ThemeToggle.tsx";

export const REPO_URL = "https://github.com/Arven-Labs/warden";

export function SiteHeader() {
  return (
    <header className="site-header">
      <Link href="/" aria-label="Warden home">
        <Logo />
      </Link>
      <nav aria-label="Main" className="site-nav">
        <Link href="/#features">Features</Link>
        <Link href="/#install">Install</Link>
        <Link href="/docs/overview/">Docs</Link>
      </nav>
      <div className="site-header-end">
        <ThemeToggle />
        <a className="btn btn-outline btn-sm" href={REPO_URL}>
          GitHub
        </a>
      </div>
    </header>
  );
}
