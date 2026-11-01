// The arguments of `browser_scroll`, checked before anything reaches the page. They come from the model, so they are not trusted: a direction
// outside the list, a distance that is not a sane number of pixels or a selector that is not text is refused here, with a message the model can
// act on. Pure, so it is tested without a browser (`tests/scrollArgs.test.mjs`); `background/tools/scroll.ts` runs the scroll itself.

export const SCROLL_DIRECTIONS = ["up", "down", "top", "bottom"] as const;
export type ScrollDirection = (typeof SCROLL_DIRECTIONS)[number];

/** The most one call moves, in pixels: a few screens. Past it the model should say `top` or `bottom`. */
export const MAX_SCROLL_PX = 10_000;

export interface ScrollArgs {
  direction: ScrollDirection;
  /** Pixels for `up` and `down`; `undefined` means about one screen, which the page measures. Always `undefined` for `top` and `bottom`. */
  amount?: number;
  selector?: string;
  tabId?: number;
}

export function parseScrollArgs(args: unknown): ScrollArgs {
  if (typeof args !== "object" || args === null || Array.isArray(args)) {
    throw new Error("the arguments of browser_scroll must be an object with a 'direction'");
  }
  const { direction, amount, selector, tabId } = args as Record<string, unknown>;

  if (typeof direction !== "string" || !(SCROLL_DIRECTIONS as readonly string[]).includes(direction)) {
    throw new Error(`'direction' must be one of ${SCROLL_DIRECTIONS.join(", ")}`);
  }
  const parsed: ScrollArgs = { direction: direction as ScrollDirection };

  if (amount !== undefined && direction !== "top" && direction !== "bottom") {
    if (typeof amount !== "number" || !Number.isFinite(amount)) {
      throw new Error("'amount' must be a number of pixels");
    }
    const pixels = Math.round(amount);
    if (pixels < 1 || pixels > MAX_SCROLL_PX) {
      throw new Error(`'amount' must be between 1 and ${MAX_SCROLL_PX} pixels; use direction 'top' or 'bottom' to go all the way`);
    }
    parsed.amount = pixels;
  }

  if (selector !== undefined) {
    if (typeof selector !== "string" || selector.trim() === "") {
      throw new Error("'selector' must be a non-empty CSS selector when provided");
    }
    parsed.selector = selector;
  }

  if (tabId !== undefined) {
    if (typeof tabId !== "number") {
      throw new Error("'tabId' argument must be a number when provided");
    }
    parsed.tabId = tabId;
  }
  return parsed;
}
