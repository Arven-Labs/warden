import Link from "next/link";
import { Logo } from "./Logo.tsx";
import { REPO_URL } from "./SiteHeader.tsx";

export function Footer() {
  return (
    <footer className="site-footer">
      <div>
        <Logo />
        <p>Open source. Your agent, on your own devices.</p>
      </div>
      <nav aria-label="Footer">
        <Link href="/docs/overview/">Documentation</Link>
        <a href={`${REPO_URL}/issues`}>Support</a>
        <a href={REPO_URL}>GitHub</a>
        <a href={`${REPO_URL}/blob/main/LICENSE`}>License</a>
      </nav>
    </footer>
  );
}
