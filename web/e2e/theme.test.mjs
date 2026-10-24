// The theme of the page (P137): dark until the person picks light, kept in this browser, and the shield is the mark. Run with `npm run test:e2e`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { launchBrowser, pageOptions, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

const background = (page) => page.evaluate(() => getComputedStyle(document.body).backgroundColor);

describe("the theme", () => {
  test(
    "opens dark even when the system is light, switches to light and keeps the choice",
    { timeout: TIMEOUT },
    async () => {
      const hub = await startHub();
      try {
        // A system in the light theme and no saved choice: the page is dark all the same.
        const context = await browser.newContext({ ...pageOptions, colorScheme: "light", locale: "en-US" });
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (e) => errors.push(e.message));
        page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
        await page.goto(hub.url);
        await page.getByRole("tab", { name: "Pairing key" }).waitFor();
        assert.equal(await background(page), "rgb(7, 6, 11)");
        assert.equal(await page.locator(".login-brand img").first().getAttribute("src"), "/favicon.svg");
        await snap(page, "theme-login-dark");

        await page.getByRole("button", { name: "Switch to the light theme" }).click();
        assert.equal(await background(page), "rgb(251, 249, 255)");
        await snap(page, "theme-login-light");

        await page.reload();
        await page.getByRole("tab", { name: "Pairing key" }).waitFor();
        assert.equal(await background(page), "rgb(251, 249, 255)", "the choice survives a reload");

        await page.getByRole("button", { name: "Switch to the dark theme" }).click();
        assert.equal(await background(page), "rgb(7, 6, 11)");
        assert.deepEqual(errors, []);
        await context.close();
      } finally {
        await hub.stop();
      }
    },
  );
});
