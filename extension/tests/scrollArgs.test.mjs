// The arguments of `browser_scroll` (`src/protocol/scrollArgs.ts`), which come from the model and so are checked before anything reaches the page.
// Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { MAX_SCROLL_PX, parseScrollArgs } from "../src/protocol/scrollArgs.ts";

describe("the direction", () => {
  test("is one of up, down, top and bottom", () => {
    for (const direction of ["up", "down", "top", "bottom"]) {
      assert.equal(parseScrollArgs({ direction }).direction, direction);
    }
  });

  test("anything else is refused, and so is its absence", () => {
    for (const direction of ["left", "DOWN", "", " down", 1, null, undefined, ["down"]]) {
      assert.throws(() => parseScrollArgs({ direction }), /direction/, String(direction));
    }
    assert.throws(() => parseScrollArgs({}), /direction/);
  });
});

describe("the amount", () => {
  test("is optional: without it the page scrolls by about one screen, which the page itself measures", () => {
    assert.equal(parseScrollArgs({ direction: "down" }).amount, undefined);
  });

  test("a positive number of pixels up to the ceiling is kept, rounded to whole pixels", () => {
    assert.equal(parseScrollArgs({ direction: "down", amount: 300 }).amount, 300);
    assert.equal(parseScrollArgs({ direction: "up", amount: 250.6 }).amount, 251);
    assert.equal(parseScrollArgs({ direction: "down", amount: MAX_SCROLL_PX }).amount, MAX_SCROLL_PX);
  });

  test("zero, negative, past the ceiling, not a number or not finite are refused", () => {
    for (const amount of [0, -5, MAX_SCROLL_PX + 1, Number.NaN, Number.POSITIVE_INFINITY, "300", null, 0.2]) {
      assert.throws(() => parseScrollArgs({ direction: "down", amount }), /amount/, String(amount));
    }
  });

  test("top and bottom have no distance: an amount sent with them is ignored, not an error", () => {
    assert.equal(parseScrollArgs({ direction: "top", amount: 300 }).amount, undefined);
    assert.equal(parseScrollArgs({ direction: "bottom", amount: -1 }).amount, undefined);
  });
});

describe("the selector and the tab", () => {
  test("the selector is optional, and a blank or non-text one is refused", () => {
    assert.equal(parseScrollArgs({ direction: "down" }).selector, undefined);
    assert.equal(parseScrollArgs({ direction: "down", selector: "#feed" }).selector, "#feed");
    for (const selector of ["", "   ", 5, null]) {
      assert.throws(() => parseScrollArgs({ direction: "down", selector }), /selector/, String(selector));
    }
  });

  test("the tab id is optional and must be a number", () => {
    assert.equal(parseScrollArgs({ direction: "down" }).tabId, undefined);
    assert.equal(parseScrollArgs({ direction: "down", tabId: 12 }).tabId, 12);
    assert.throws(() => parseScrollArgs({ direction: "down", tabId: "12" }), /tabId/);
  });
});

describe("the arguments as a whole", () => {
  test("must be an object", () => {
    for (const args of [null, undefined, "down", 5, ["down"]]) {
      assert.throws(() => parseScrollArgs(args), /arguments/, String(args));
    }
  });
});
