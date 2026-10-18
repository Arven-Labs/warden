// What the end-to-end tests of the desktop screens need: the built page (`desktop/dist`) served from a folder, a headless browser, and a
// pretend Tauri backend. There is no `tauri-driver` here: the page's `invoke` is answered by the test with invented data, the way
// `@tauri-apps/api/mocks` (`mockIPC`) does it, so what is proved is the screen and what it asks for, not the Rust behind it.
//
// Needs, once: `npm run build` (this folder's `dist`) and `npx playwright-core install chromium`.

import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const desktopRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const distDir = path.join(desktopRoot, "dist");

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".json": "application/json",
  ".woff2": "font/woff2",
};

/** Serves `desktop/dist` on a free port; `stop()` closes it. */
export async function serveDist() {
  if (!fs.existsSync(path.join(distDir, "index.html"))) {
    throw new Error(`There is no build at ${distDir}. Run \`npm run build\` in desktop/ first.`);
  }
  const server = http.createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");
    const file = path.join(distDir, path.normalize(decodeURIComponent(url.pathname)));
    const inside = file.startsWith(distDir) && fs.existsSync(file) && fs.statSync(file).isFile();
    const served = inside ? file : path.join(distDir, "index.html");
    res.writeHead(200, { "content-type": TYPES[path.extname(served)] ?? "application/octet-stream" });
    fs.createReadStream(served).pipe(res);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}/`,
    stop: () => new Promise((resolve) => server.close(resolve)),
  };
}

export async function launchBrowser() {
  const executablePath = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE;
  const attempts = executablePath ? [{ executablePath }] : [{}, { channel: "chrome" }];
  let last;
  for (const options of attempts) {
    try {
      return await chromium.launch(options);
    } catch (err) {
      last = err;
    }
  }
  throw new Error(`There is no browser to drive. Install one with \`npx playwright-core install chromium\`, or point PLAYWRIGHT_CHROMIUM_EXECUTABLE at Chrome or Chromium.\n${last?.message ?? ""}`);
}

/** The pretend backend inside the page: the shape `@tauri-apps/api` looks for, with every `invoke` handed to the test. */
const BACKEND = `(() => {
  const callbacks = new Map();
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    invoke: (cmd, args) => window.__warden_invoke(cmd, args ?? {}),
    transformCallback: (callback, once) => {
      const id = window.crypto.getRandomValues(new Uint32Array(1))[0];
      callbacks.set(id, (data) => {
        if (once) callbacks.delete(id);
        return callback && callback(data);
      });
      return id;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    runCallback: (id, data) => callbacks.get(id)?.(data),
    callbacks,
    convertFileSrc: (path) => path,
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event, id) => callbacks.delete(id) };
})();`;

/** A promise the test settles later: how a command that waits for the phone ("sync_push_await") stays waiting while its screen is photographed. */
export function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/**
 * Opens the page with `handlers` answering the backend: a command name to a value, or to a function of its arguments (that may return a
 * promise). A command with no handler gets `[]` when it is a `list_...` and `null` otherwise. Returns the page, every call made
 * (`calls`: `{ cmd, args }`) and the errors the page raised that nothing caught (`errors`), which a test asserts empty.
 */
export async function openApp(browser, app, { handlers = {}, colorScheme = "light" } = {}) {
  const context = await browser.newContext({ colorScheme, viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const calls = [];
  const errors = [];
  page.on("pageerror", (err) => errors.push(err.message));
  await page.exposeFunction("__warden_invoke", async (cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "plugin:event|listen") return args.handler;
    if (cmd.startsWith("plugin:event|")) return null;
    const handler = handlers[cmd];
    if (handler === undefined) return cmd.startsWith("list_") ? [] : null;
    return typeof handler === "function" ? handler(args) : handler;
  });
  await page.addInitScript(BACKEND);
  await page.goto(app.url);
  return { context, page, calls, errors };
}

const screensDir = () => process.env.WARDEN_E2E_SCREENS ?? path.join(desktopRoot, "e2e-screens");

/**
 * Photographs the page as `<name>.png` for someone to look at afterwards (the CI uploads the folder as an artifact, always).
 * Call it once the screen is ready, one per interesting state. `focus` is a locator to scroll into view first, and `top` puts it at the
 * top of its scroller instead of only into view (a heading half in view would otherwise not move, and the picture would be of what
 * was above it). It never fails the test: a picture is for the eye, the assertions stay on text. Only invented data may be on screen.
 */
export async function snap(page, name, { focus, top = false } = {}) {
  try {
    if (focus && top) await focus.evaluate((el) => el.scrollIntoView({ block: "start" }));
    else if (focus) await focus.scrollIntoViewIfNeeded();
    fs.mkdirSync(screensDir(), { recursive: true });
    await page.screenshot({ path: path.join(screensDir(), `${name}.png`) });
  } catch (err) {
    console.warn(`snap(${name}) did not work: ${err?.message ?? err}`);
  }
}
