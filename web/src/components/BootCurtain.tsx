import { useEffect, useState } from "react";
import {
  BOOT_CURTAIN_FADE_MS,
  BOOT_CURTAIN_MAXIMUM_MS,
  BOOT_CURTAIN_MINIMUM_MS,
  BOOT_CURTAIN_SETTLE_MS,
  bootPhase,
  type BootInputs,
  usePendingSurfaceCount,
} from "../bootReadiness";
import { RfLoader } from "./RfLoader";

/**
 * The RackForge mark, full screen and centred, while the app gets ready.
 *
 * It covers what a cold start used to show one after another -- "no
 * instrument active" before the session arrived, the page's loader, the
 * plugin's own -- and lifts, with a fade, once there is something to play.
 */
export function BootCurtain(props: Omit<BootInputs, "pendingSurfaces">) {
  const pendingSurfaces = usePendingSurfaceCount();
  const phase = bootPhase({ ...props, pendingSurfaces });
  const [mountedAt] = useState(() => performance.now());
  const [stage, setStage] = useState<"shown" | "leaving" | "gone">("shown");

  // Leave once ready has held for the settle window and the minimum has
  // passed -- or once the maximum has, whatever is pending.
  useEffect(() => {
    if (stage !== "shown") return;
    const elapsed = performance.now() - mountedAt;
    const delay = phase.ready
      ? Math.max(BOOT_CURTAIN_SETTLE_MS, BOOT_CURTAIN_MINIMUM_MS - elapsed)
      : BOOT_CURTAIN_MAXIMUM_MS - elapsed;
    const timer = window.setTimeout(() => setStage("leaving"), Math.max(0, delay));
    return () => window.clearTimeout(timer);
  }, [mountedAt, phase.ready, stage]);

  // Unmount when the fade is over. A timer rather than `transitionend`,
  // which never fires when the fade is switched off for reduced motion.
  useEffect(() => {
    if (stage !== "leaving") return;
    const timer = window.setTimeout(() => setStage("gone"), BOOT_CURTAIN_FADE_MS);
    return () => window.clearTimeout(timer);
  }, [stage]);

  if (stage === "gone") return null;
  return (
    <div
      className={`boot-curtain${stage === "leaving" ? " boot-curtain--leaving" : ""}`}
      aria-hidden={stage === "leaving" ? true : undefined}
    >
      <RfLoader label="RackForge" detail={phase.detail || "Ready"} size="large" />
    </div>
  );
}
