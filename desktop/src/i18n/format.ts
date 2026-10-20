import { intlTag } from "./index.ts";

/** "14:05" today, "12/09" before that — enough to tell recent conversations apart. */
export function shortDate(millis: number): string {
  const date = new Date(millis);
  return date.toDateString() === new Date().toDateString()
    ? date.toLocaleTimeString(intlTag(), { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString(intlTag(), { day: "2-digit", month: "2-digit" });
}
