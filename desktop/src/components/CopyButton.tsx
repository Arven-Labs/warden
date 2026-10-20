import { useEffect, useRef, useState } from "react";
import { t } from "../i18n";
import { copyText } from "../lib/clipboard";

interface CopyButtonProps {
  /** What is copied: a function reads it at the moment of the click (a long message is not turned into text before anyone asks). */
  text: string | (() => string);
  className?: string;
  label?: string;
}

/** "Copy", which says "Copied" for a moment after it worked, or "Could not copy" when nothing let it. */
function CopyButton({ text, className = "copy-btn", label }: CopyButtonProps) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => () => clearTimeout(timer.current), []);

  async function handleClick() {
    const ok = await copyText(typeof text === "function" ? text() : text);
    setState(ok ? "copied" : "failed");
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("idle"), 1500);
  }

  return (
    <button type="button" className={className} onClick={() => void handleClick()}>
      {state === "copied" ? t("copy.copied") : state === "failed" ? t("copy.failed") : label ?? t("copy.copy")}
    </button>
  );
}

export default CopyButton;
