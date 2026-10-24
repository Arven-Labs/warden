import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import "./tokens.css";
import "./globals.css";

const BASE_PATH = process.env.SITE_BASE_PATH ?? "";
// Absolute origin for the share image; a custom domain would replace it.
const ORIGIN = process.env.SITE_ORIGIN ?? "https://arven-labs.github.io";
const DESCRIPTION = "Warden is a personal AI agent that lives on your own devices. Install it, run it, own it.";

export const metadata: Metadata = {
  metadataBase: new URL(ORIGIN),
  title: { default: "Warden", template: "%s · Warden" },
  description: DESCRIPTION,
  openGraph: {
    title: "Warden",
    description: DESCRIPTION,
    type: "website",
    images: [{ url: `${BASE_PATH}/brand/og.png`, width: 1200, height: 630 }],
  },
  twitter: { card: "summary_large_image", images: [`${BASE_PATH}/brand/og.png`] },
};

export const viewport: Viewport = { themeColor: "#07060b" };

// Applies the saved theme before the first paint so the page doesn't flash dark-to-light.
const THEME_SCRIPT = `try{var t=localStorage.getItem("warden-theme");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}catch(e){}`;

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
      </head>
      <body>{children}</body>
    </html>
  );
}
