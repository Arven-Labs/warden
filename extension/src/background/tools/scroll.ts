import type { ToolSpec } from "../../protocol/messages";
import { parseScrollArgs, type ScrollDirection } from "../../protocol/scrollArgs";
import { runInPage } from "./dom_executor";

export const scrollSpec: ToolSpec = {
  name: "browser_scroll",
  description:
    "Scrolls a browser tab, or one scrollable element in it, so that content below or above the visible part can be read with " +
    "browser_read_page or browser_extract_text (they only see what is on screen). direction is up, down, top or bottom. For up and down, " +
    "amount is a number of pixels (default: about one screen). It only moves the view: it does not click or type. Returns where the " +
    "scroll ended and whether the top or the bottom was reached.",
  parameters: {
    type: "object",
    properties: {
      direction: { type: "string", enum: ["up", "down", "top", "bottom"], description: "Where to scroll to." },
      amount: { type: "number", description: "Pixels to scroll for up and down (1 to 10000). Omit for about one screen." },
      selector: { type: "string", description: "CSS selector of a scrollable element to scroll instead of the page (a feed, a chat list, a panel)." },
      tabId: { type: "number", description: "Tab id from browser_list_tabs to act on — omit to use whichever tab is currently active/focused." },
    },
    required: ["direction"],
  },
};

interface ScrollResult {
  /** Pixels from the top after the scroll. */
  scrollTop: number;
  /** Pixels the view actually moved (0 when it was already at the end, or the element does not scroll). */
  scrolledBy: number;
  atTop: boolean;
  atBottom: boolean;
  /** The whole height of what can be scrolled, and the height of the part in view. */
  scrollHeight: number;
  viewportHeight: number;
}

/** Runs inside the page — must be fully self-contained, see `dom_executor.ts`. */
function scrollInPage(direction: ScrollDirection, amount: number | null, selector: string | null): ScrollResult {
  const element = selector === null ? (document.scrollingElement ?? document.documentElement) : document.querySelector(selector);
  if (!element) {
    throw new Error(`no element matches selector '${selector}'`);
  }
  const viewportHeight = selector === null ? window.innerHeight : element.clientHeight;
  const maxTop = Math.max(0, element.scrollHeight - viewportHeight);
  const step = amount ?? Math.max(1, Math.round(viewportHeight * 0.8));
  const before = element.scrollTop;
  let target = before;
  if (direction === "up") target = before - step;
  else if (direction === "down") target = before + step;
  else if (direction === "top") target = 0;
  else target = maxTop;
  element.scrollTop = Math.min(maxTop, Math.max(0, target));
  const after = element.scrollTop;
  return {
    scrollTop: Math.round(after),
    scrolledBy: Math.round(after - before),
    atTop: after <= 0,
    atBottom: after >= maxTop - 1,
    scrollHeight: element.scrollHeight,
    viewportHeight,
  };
}

export async function scroll(args: unknown): Promise<ScrollResult> {
  const { direction, amount, selector, tabId } = parseScrollArgs(args);
  return runInPage(scrollInPage, [direction, amount ?? null, selector ?? null], tabId);
}
