import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { PerformanceInfoBar } from "../components/PerformanceInfoBar";
import { PlayChainDrawer } from "../components/PlayChainDrawer";
import { PluginFrame } from "../components/PluginFrame";
import { PluginIcon } from "../components/PluginIcon";
import { PluginSurfaceState } from "../components/PluginSurfaceState";
import { RfLoader } from "../components/RfLoader";
import { PluginPickerModal } from "../dialogs/PluginPickerModal";
import { PresetModal } from "../dialogs/PresetModal";
import { dispatchCommandAwait } from "../gateway";
import { type PlayChain, chainOf, sameChain } from "../playChain";
import { usePluginCatalog } from "../pluginCatalog";
import { formatPluginVersion } from "../pluginPresentation";
import { type SessionSnapshot } from "../types";

export function PlayPage({
  snapshot,
  overlay,
  onOverlayChange,
  preferredInstanceId,
}: {
  snapshot: SessionSnapshot | null;
  overlay: "plugins" | "presets" | null;
  onOverlayChange: (overlay: "plugins" | "presets" | null) => void;
  preferredInstanceId?: string | null;
}) {
  const instances = snapshot?.instances ?? [];
  const active =
    instances.find((instance) => instance.instance_id === preferredInstanceId) ??
    instances.find(
      (instance) => instance.instance_id === snapshot?.active_instance_id,
    ) ?? instances[0];
  const playActivationStartedRef = useRef(false);
  useEffect(() => {
    if (
      playActivationStartedRef.current ||
      !snapshot ||
      (snapshot.active_mode === "play" && snapshot.live.active === undefined)
    ) return;
    playActivationStartedRef.current = true;
    const instanceId = active?.instance_id;
    void (async () => {
      try {
        // Rack preview cleanup is dispatched while this route mounts. Awaiting
        // the PLAY transition here makes it the final owner of the audio path,
        // then re-applies the already selected standalone instrument state.
        const applied = await dispatchCommandAwait({
          type: "set_active_mode",
          mode: "play",
        });
        // Leaving LIVE hands the voice back: the host puts the instrument
        // PLAY was holding before a Rack borrowed it. This route read
        // `instanceId` while LIVE still owned the voice, so re-asserting it
        // here is how the Rack's instrument followed the player home. If the
        // host already chose, that choice is the newer truth.
        const hostChose = applied.events.some(
          (event) =>
            typeof event === "object"
            && event !== null
            && (event as { event?: { type?: string } }).event?.type
              === "active_instance_changed",
        );
        if (hostChose || !instanceId) return;
        await dispatchCommandAwait({ type: "select_plugin", instance_id: instanceId });
        // Deliberately not re-selecting the program. `soundId` was read from
        // the session, so re-applying it can only ever restate what the host
        // already holds — and selecting a program loads its preset, which
        // overwrites every parameter the player has touched since. Editing a
        // control, going to the on-screen keyboard and coming back reset the
        // instrument to the preset. Neither `set_active_mode` nor
        // `select_plugin` disturbs the selection, so nothing here needs it.
      } catch {
        // The gateway publishes command failures through the shared error
        // banner. A later explicit program selection remains a safe retry.
      }
    })();
  }, [active, snapshot]);
  const pluginPickerOpen = overlay === "plugins";
  const presetsOpen = overlay === "presets";
  const [surfaceInfo, setSurfaceInfo] = useState<{
    instanceId: string;
    label: string;
    value: string;
  } | null>(null);
  const pluginCatalog = usePluginCatalog();
  const { plugins: installedPlugins } = pluginCatalog;
  const activeVersion = installedPlugins.find(
    (plugin) => plugin.plugin_id === active?.plugin_id,
  )?.version;
  const activeDescriptor = installedPlugins.find(
    (plugin) => plugin.plugin_id === active?.plugin_id,
  );
  const activeProgram = active?.sounds.find(
    (sound) => sound.id === active.selected_sound_id,
  );
  const activeSurfaceInfo =
    surfaceInfo?.instanceId === active?.instance_id ? surfaceInfo : null;
  const activeInstanceId = active?.instance_id;
  // The chain is the instrument's and the host's: the session holds one
  // per instrument and routes PLAY through it. An edit shows at once and
  // is sent; it stands until the session answers -- with the same chain,
  // or with another (refused, or changed elsewhere), which then wins.
  const [chainOpen, setChainOpen] = useState(false);
  const [pendingChain, setPendingChain] = useState<{
    chain: PlayChain;
    revision: number;
  } | null>(null);
  const snapshotChains = snapshot?.play_chains;
  const snapshotRevision = snapshot?.revision;
  const sessionChain = useMemo(
    () => (activeInstanceId ? chainOf(snapshotChains, activeInstanceId) : null),
    [activeInstanceId, snapshotChains],
  );
  const chain =
    pendingChain
    && sessionChain
    && pendingChain.chain.instrument_id === sessionChain.instrument_id
    && (pendingChain.revision === snapshotRevision || sameChain(pendingChain.chain, sessionChain))
      ? pendingChain.chain
      : sessionChain;
  // One effect's panel at a time, inside the drawer. The frame addresses
  // the effect's own instance, which both hosts run beside the instrument
  // and let through their parameter gates while it is on stage.
  const [openEffectId, setOpenEffectId] = useState<string | null>(null);
  const openEffect = chain?.effects.find((effect) => effect.id === openEffectId) ?? null;
  const openEffectInstance = openEffect
    ? instances.find((instance) => instance.plugin_id === openEffect.plugin_id) ?? null
    : null;
  const effectPanel =
    active && openEffect && openEffectInstance ? (
      <PluginFrame
        key={`${active.instance_id}.fx.${openEffect.id}`}
        instance={{
          ...openEffectInstance,
          instance_id: `${active.instance_id}.fx.${openEffect.id}`,
          // The program is the chain's, not the standalone instance's: the
          // same effect can sit in the chain twice on two settings.
          selected_sound_id: openEffect.program_id
            ?? openEffectInstance.selected_sound_id,
        }}
        surface="play"
        onSelectSound={(soundId) =>
          dispatchCommandAwait({
            type: "select_sound",
            instance_id: `${active.instance_id}.fx.${openEffect.id}`,
            sound_id: soundId,
          })}
      />
    ) : null;
  const handleChainChange = useCallback(
    (next: PlayChain) => {
      setPendingChain({ chain: next, revision: snapshotRevision ?? -1 });
      void dispatchCommandAwait({
        type: "set_play_chain",
        instrument_id: next.instrument_id,
        effects: next.effects,
      }).catch(() => setPendingChain(null));
    },
    [snapshotRevision],
  );
  const handleSurfaceInfo = useCallback(
    (info: { label: string; value: string } | null) => {
      if (!activeInstanceId) return;
      setSurfaceInfo(
        info
          ? { instanceId: activeInstanceId, ...info }
          : null,
      );
    },
    [activeInstanceId],
  );
  return (
    <section className="plugin-surface-shell direct-surface">
      <div className="play-plugin-toolbar">
        <button
          className={`play-header-button back${pluginPickerOpen ? " active" : ""}`}
          onClick={() => {
            onOverlayChange(pluginPickerOpen ? null : "plugins");
          }}
          aria-expanded={pluginPickerOpen}
        >
          <span aria-hidden="true">▦</span>
          <strong>Select plugin</strong>
        </button>
        <PerformanceInfoBar
          className="play-plugin-identity"
          left={{ label: "Mode", value: "PLAY" }}
          center={{
            label: activeSurfaceInfo?.label || "Program",
            value: activeSurfaceInfo?.value || activeProgram?.name || "No program",
          }}
          right={{
            label: "Plugin",
            value: active
              ? `${active.plugin_name}${formatPluginVersion(activeVersion)}`
              : "Select an instrument",
          }}
          rightAccessory={
            active ? (
              <PluginIcon
                plugin={activeDescriptor}
                name={active.plugin_name}
                className="play-plugin-icon"
              />
            ) : null
          }
        />
        <div className="play-plugin-actions">
          <button
            className={`play-header-button chain${chainOpen ? " active" : ""}`}
            disabled={!active}
            onClick={() => setChainOpen((state) => !state)}
            aria-expanded={chainOpen}
            aria-controls="play-chain"
          >
            <span className="fx-button-mark" aria-hidden="true">FX</span>
            <strong>Effects</strong>
          </button>
          <button
            className={`play-header-button presets${presetsOpen ? " active" : ""}`}
            disabled={!active}
            onClick={() => {
              onOverlayChange(presetsOpen ? null : "presets");
            }}
            aria-expanded={presetsOpen}
          >
            <span className="preset-button-mark" aria-hidden="true">P</span>
            <strong>Presets</strong>
          </button>
        </div>
      </div>
      {/* Always in the tree and always before the stage: opening it never moves the iframe. */}
      <PlayChainDrawer
        open={chainOpen && chain !== null}
        chain={chain ?? { instrument_id: "", effects: [] }}
        plugins={installedPlugins}
        instances={instances}
        instrumentName={active?.plugin_name ?? "Instrument"}
        instrumentVersion={activeVersion}
        instrumentDescriptor={activeDescriptor}
        suggested={activeDescriptor?.suggested_chain}
        onChange={handleChainChange}
        onClose={() => setChainOpen(false)}
        openEffectId={openEffectId}
        onOpenEffect={setOpenEffectId}
        effectPanel={effectPanel}
      />
      {active ? (
        <PluginFrame
          key={active.instance_id}
          instance={active}
          surface="play"
          onSurfaceInfoChange={handleSurfaceInfo}
        />
      ) : !snapshot ? (
        // No session yet is not "no instrument": the one that is playing
        // simply has not been reported.
        <RfLoader
          className="plugin-play-loader"
          label="Connecting to RackForge"
          detail="Waiting for the session…"
          size="large"
        />
      ) : pluginCatalog.status === "idle" || pluginCatalog.status === "loading" ? (
        <RfLoader
          className="plugin-play-loader"
          label="Loading PLAY instruments"
          detail="Waiting for the plugin catalog and audio runtime…"
          size="large"
        />
      ) : (
        <PluginSurfaceState
          title="No instrument active"
          detail="Select one of the installed RackForge plugins to start playing."
        />
      )}
      {pluginPickerOpen && (
        <PluginPickerModal
          active={active}
          instances={instances}
          plugins={installedPlugins}
          programDraft={snapshot?.program_draft}
          onClose={() => onOverlayChange(null)}
        />
      )}
      {presetsOpen && active && (
        <PresetModal
          key={active.instance_id}
          instance={active}
          onClose={() => onOverlayChange(null)}
        />
      )}
    </section>
  );
}
