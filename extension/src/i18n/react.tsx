import { Fragment, useSyncExternalStore, type ReactNode } from "react";
import { locale, subscribe, t, type Key, type Locale } from "./index.ts";

/** The current language for a component, which draws again when the person switches it. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, locale);
}

/** Like `t`, for a text that has an element inside it (a `<code>`, a `<strong>`): each `{name}` is replaced by the node given for it. */
export function rich(key: Key, nodes: Record<string, ReactNode>): ReactNode {
  return t(key)
    .split(/(\{\w+\})/)
    .map((part, i) => {
      const name = /^\{(\w+)\}$/.exec(part)?.[1];
      return <Fragment key={i}>{name !== undefined && name in nodes ? nodes[name] : part}</Fragment>;
    });
}
