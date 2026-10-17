import { useEffect, useRef, useState } from "react";
import { copyText } from "../lib/clipboard";

interface CopyButtonProps {
  /** What is copied: a function reads it at the moment of the click (a long message is not turned into text before anyone asks). */
  text: string | (() => string);
  className?: string;
  label?: string;
}

/** "Copiar", which says "Copiado" for a moment after it worked, or "Não copiou" when nothing let it. */
function CopyButton({ text, className = "copy-btn", label = "Copiar" }: CopyButtonProps) {
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
      {state === "copied" ? "Copiado" : state === "failed" ? "Não copiou" : label}
    </button>
  );
}

export default CopyButton;
