import { useEffect, useState } from "react";
import { useMediaQuery } from "../hooks/useMediaQuery";

/**
 * The lighting condition the interface is actually rendering in.
 *
 * Plugins are told the condition, never the preference. "Auto" is a statement
 * about *who decides*, which is the host's business; a plugin surface only
 * needs to know which of the two it is being drawn next to. So this resolves
 * auto and reports "day" or "stage".
 *
 * Two things can change it — the player throwing the switch, which stamps the
 * document, and the operating system flipping underneath an unstamped one —
 * so both are watched.
 */
export function useResolvedLighting(): "day" | "stage" {
  const systemDark = useMediaQuery("(prefers-color-scheme: dark)");
  const [stamp, setStamp] = useState<string | null>(() =>
    document.documentElement.getAttribute("data-theme"),
  );
  useEffect(() => {
    const root = document.documentElement;
    const observer = new MutationObserver(() =>
      setStamp(root.getAttribute("data-theme")),
    );
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);
  if (stamp === "dark") return "stage";
  if (stamp === "light") return "day";
  return systemDark ? "stage" : "day";
}
