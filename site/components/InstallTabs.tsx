"use client";

import { useState } from "react";

/** Commands come from GUIA-DE-USO.md; `tests/landing.test.mjs` checks they still appear there. */
export const INSTALL_TABS = [
  {
    id: "linux",
    label: "Linux",
    code: `# Debian, Ubuntu (download the .deb from the latest release)
sudo apt install ./Warden_*_amd64.deb

# Any Linux
chmod +x Warden_*.AppImage && ./Warden_*.AppImage`,
  },
  {
    id: "docker",
    label: "Docker",
    code: `# An always-on hub for your phone and browser
docker volume create warden-data
docker run -d --name warden --restart unless-stopped \\
  --read-only --tmpfs /tmp -v warden-data:/data -p 7420:7420 \\
  -e WARDEN_SERVER_AUTH_KEY="$KEY" \\
  ghcr.io/arven-labs/warden-server:latest serve --no-embedding-warm-up --run-tasks`,
  },
  {
    id: "other",
    label: "Windows, macOS",
    code: `# Download the installer for your system from the latest release
#   Windows: Warden_<version>_x64_en-US.msi
#   macOS (Apple Silicon): Warden_<version>_aarch64.dmg
# Installers are not signed yet: SmartScreen and Gatekeeper will warn.`,
  },
] as const;

export function InstallTabs() {
  const [active, setActive] = useState<string>(INSTALL_TABS[0].id);
  const [copied, setCopied] = useState(false);
  const tab = INSTALL_TABS.find((t) => t.id === active) ?? INSTALL_TABS[0];

  async function copy() {
    try {
      await navigator.clipboard.writeText(tab.code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // clipboard needs a secure context; the text stays selectable
    }
  }

  return (
    <div className="codebox">
      <div className="codebox-tabs" role="tablist">
        {INSTALL_TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={t.id === active}
            onClick={() => setActive(t.id)}
          >
            {t.label}
          </button>
        ))}
        <button type="button" className="codebox-copy" onClick={copy}>
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre>
        <code>{tab.code}</code>
      </pre>
    </div>
  );
}
