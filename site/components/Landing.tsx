import Link from "next/link";
import { Footer } from "./Footer.tsx";
import { InstallTabs } from "./InstallTabs.tsx";
import { LogoMark } from "./Logo.tsx";
import { REPO_URL, SiteHeader } from "./SiteHeader.tsx";

const FEATURES = [
  { title: "Your vault, your files", text: "Memory lives in a plain markdown vault you own. No lock-in, no cloud copy you can't see." },
  { title: "Everywhere you are", text: "Desktop, phone, terminal, browser, Telegram and WhatsApp talk to the same agent." },
  { title: "Keys stay with you", text: "Model keys and secrets stay in the config and vault on your own devices, not on a server you don't control." },
  { title: "Always on, when you want", text: "Run an always-on hub so scheduled tasks keep going while your computer is off." },
] as const;

const HELP = [
  { title: "Documentation", text: "Guides from first install to the hub, tasks, nodes and the API.", href: "/docs/overview/", label: "Read the docs" },
  { title: "Support", text: "Found a bug or stuck on a step? Open an issue and we'll look.", href: `${REPO_URL}/issues`, label: "Open an issue" },
  { title: "Source", text: "Read the code, build it yourself, send a patch.", href: REPO_URL, label: "View on GitHub" },
] as const;

function AppWindow() {
  return (
    <div className="window" aria-hidden="true">
      <div className="window-bar">
        <i /> <i /> <i />
      </div>
      <div className="window-body">
        <aside>
          <span className="window-brand">
            <LogoMark size={18} /> Warden
          </span>
          <span className="active">Chat</span>
          <span>Agents</span>
          <span>Vault</span>
          <span>Tasks</span>
        </aside>
        <div className="window-chat">
          <p className="bubble bubble-user">What's on my plate today?</p>
          <p className="bubble">Two tasks are due, and your backup ran at 03:00. Want the summary?</p>
          <p className="window-input">Message your agent…</p>
        </div>
      </div>
    </div>
  );
}

export function Landing() {
  return (
    <>
      <SiteHeader />
      <main className="landing">
        <section className="hero">
          <div>
            <span className="pill">
              <b>Open source</b> · runs on your devices
            </span>
            <h1>
              Your agent, <span className="hl">yours</span> to keep.
            </h1>
            <p className="lead">
              Warden is a personal AI agent that lives on your own devices. It remembers in a markdown vault you
              own and reaches you wherever you already are.
            </p>
            <div className="cta">
              <Link className="btn btn-primary" href="/docs/overview/">
                Get started →
              </Link>
              <a className="btn btn-outline" href={REPO_URL}>
                View on GitHub
              </a>
            </div>
            <ul className="ticks">
              <li>100% open source</li>
              <li>Privacy first</li>
              <li>Modular</li>
            </ul>
          </div>
          <AppWindow />
        </section>

        <section id="features" className="band band-light">
          <div className="band-grid">
            <div>
              <span className="eyebrow">Why Warden</span>
              <h2>Everything in one agent, none of it rented.</h2>
              <p className="muted">
                One agent across your devices, with the memory and the keys kept where you can see them.
              </p>
            </div>
            <ul className="features">
              {FEATURES.map((f) => (
                <li key={f.title}>
                  <h3>{f.title}</h3>
                  <p>{f.text}</p>
                </li>
              ))}
            </ul>
          </div>
        </section>

        <section id="install" className="band">
          <div className="band-grid">
            <div>
              <span className="eyebrow">Start in minutes</span>
              <h2>Simple to install. Powerful to use.</h2>
              <p className="muted">
                Install the desktop app, or run the hub in Docker and connect from anywhere. Start with one and add
                the other later.
              </p>
              <Link className="btn btn-outline" href="/docs/overview/">
                Read the docs
              </Link>
            </div>
            <InstallTabs />
          </div>
        </section>

        <section className="band band-glow">
          <div className="band-grid">
            <div>
              <span className="eyebrow">Docs and support</span>
              <h2>Need a hand? We're here.</h2>
            </div>
            <ul className="glass-cards">
              {HELP.map((h) => (
                <li key={h.title}>
                  <h3>{h.title}</h3>
                  <p>{h.text}</p>
                  {h.href.startsWith("/") ? <Link href={h.href}>{h.label} →</Link> : <a href={h.href}>{h.label} →</a>}
                </li>
              ))}
            </ul>
          </div>
        </section>
      </main>
      <Footer />
    </>
  );
}
