import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import type { PluginInstance, PluginWebDescriptor } from "../types";
import {
  effectPlugins,
  isAddableSuggestion,
  suggestedEffects,
  withEffect,
  withSuggestedEffects,
  withEffectEnabled,
  withEffectMoved,
  withoutEffect,
  type PlayChain,
  type SuggestedChainEntry,
} from "../playChain";
import { PluginIcon } from "./PluginIcon";

const HEIGHT_STORAGE_KEY = "rackforge.play.chain-height.v1";
/** The height the player chose with an effect's panel open: its own memory. */
const PANEL_HEIGHT_STORAGE_KEY = "rackforge.play.chain-height-panel.v1";
/** The head and the grip alone. */
const MINIMUM_HEIGHT = 64;
/** What the plugin keeps below the drawer at the drawer's tallest. */
const STAGE_MINIMUM = 160;
/** With an effect's panel open the drawer may take nearly everything: the
 * panel is shown whole and it is the instrument that is pushed down. */
const STAGE_SLIVER = 40;
/** An effect's panel is never shorter than this. */
const PANEL_MINIMUM = 380;
/** How often an open panel's content height is read back from its document. */
const PANEL_MEASURE_MS = 400;
/** A close whose `transitionend` never came (a hidden tab, a 0 ms motion) still settles. */
const SETTLE_FALLBACK_MS = 600;

function storedHeight(key: string): number | undefined {
  try {
    const stored = JSON.parse(window.localStorage.getItem(key) ?? "null");
    return typeof stored === "number" && Number.isFinite(stored) ? stored : undefined;
  } catch {
    return undefined;
  }
}

function storeHeight(key: string, height: number | undefined) {
  try {
    if (height === undefined) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, JSON.stringify(height));
  } catch {
    // Persistent sizing is optional in hardened or ephemeral WebViews.
  }
}

/**
 * The height a plugin's surface wants: its document's content, not the
 * viewport it was given. A surface that fills its window (`html, body
 * { height: 100% }`) reports the window back as its scrollHeight, so the
 * body's children are summed instead -- a scrolling `main` inside such a
 * surface reports its whole content that way. Same-origin, so readable.
 */
function surfaceContentHeight(frame: HTMLIFrameElement | null): number | null {
  const body = frame?.contentDocument?.body;
  const view = frame?.contentWindow;
  if (!body || !view || body.children.length === 0) return null;
  const style = view.getComputedStyle(body);
  let children = (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0);
  for (const child of body.children) {
    if (child.tagName === "SCRIPT" || child.tagName === "STYLE") continue;
    const childStyle = view.getComputedStyle(child);
    children +=
      child.scrollHeight
      + (parseFloat(childStyle.marginTop) || 0)
      + (parseFloat(childStyle.marginBottom) || 0);
  }
  return Math.ceil(Math.max(children, body.scrollHeight));
}

type Phase = "closed" | "open" | "closing";

/**
 * The chain drawer: a row between the PLAY toolbar and the plugin's stage
 * that holds the instrument, the effects after it and the output.
 *
 * It is sized the way the touch controller's dock is: a row of explicit
 * height, a grip on its edge (`role="separator"`, pointer capture, the
 * arrow keys, a double-click back to the automatic height) and the height
 * kept in local storage. The plugin's stage is a flex sibling, so the row
 * pushes it by layout -- open and close are the row's height going to and
 * from nought under the host's emphasized motion, and a drag is the same
 * push without the motion. The iframe is never remounted: the drawer is a
 * sibling of the stage, never its parent.
 */
export function PlayChainDrawer({
  open,
  chain,
  plugins,
  instances,
  instrumentName,
  instrumentVersion,
  instrumentDescriptor,
  suggested,
  onChange,
  onClose,
  openEffectId = null,
  onOpenEffect,
  effectPanel,
}: {
  open: boolean;
  chain: PlayChain;
  plugins: PluginWebDescriptor[];
  /** The session's instances: an effect the host has not loaded cannot join. */
  instances?: PluginInstance[];
  instrumentName: string;
  instrumentVersion?: string;
  instrumentDescriptor?: PluginWebDescriptor;
  suggested?: SuggestedChainEntry[];
  onChange: (chain: PlayChain) => void;
  onClose: () => void;
  /** The effect whose panel is open in the drawer, if one is. */
  openEffectId?: string | null;
  onOpenEffect?: (effectId: string | null) => void;
  /** That effect's panel, mounted by the page (it owns the plugin frames). */
  effectPanel?: ReactNode;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  // A close is "closing" until the row's height transition has ended
  // (or the fallback fires: hidden tabs get no transition events).
  const [settled, setSettled] = useState(!open);
  const [seenOpen, setSeenOpen] = useState(open);
  if (seenOpen !== open) {
    setSeenOpen(open);
    setSettled(false);
  }
  const phase: Phase = open ? "open" : settled ? "closed" : "closing";
  const [chosenHeight, setChosenHeight] = useState<number | undefined>(() =>
    storedHeight(HEIGHT_STORAGE_KEY));
  const [chosenPanelHeight, setChosenPanelHeight] = useState<number | undefined>(() =>
    storedHeight(PANEL_HEIGHT_STORAGE_KEY));
  const [panelHeight, setPanelHeight] = useState(PANEL_MINIMUM);
  const bandRef = useRef<HTMLDivElement | null>(null);
  const [naturalHeight, setNaturalHeight] = useState(MINIMUM_HEIGHT);
  const [maximumHeight, setMaximumHeight] = useState(Number.POSITIVE_INFINITY);
  const [resizing, setResizing] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const gestureRef = useRef<{ pointerId: number; startY: number; height: number } | null>(null);

  useEffect(() => {
    if (phase !== "closing") return;
    const fallback = window.setTimeout(() => setSettled(true), SETTLE_FALLBACK_MS);
    return () => window.clearTimeout(fallback);
  }, [phase]);

  // The automatic height is the content's own; the ceiling leaves the
  // plugin its minimum. Both are re-measured when the content or the
  // window changes.
  const panelOpen = effectPanel != null;
  const measure = useCallback(() => {
    const content = contentRef.current;
    const host = hostRef.current;
    if (content) setNaturalHeight(Math.max(MINIMUM_HEIGHT, content.scrollHeight + GRIP_HEIGHT));
    const shell = host?.parentElement;
    if (shell) {
      // The shell is the toolbar, this row and the stage: the ceiling is
      // what the toolbar leaves, less the strip the stage keeps.
      const toolbar = host.previousElementSibling?.clientHeight ?? 0;
      const stage = panelOpen ? STAGE_SLIVER : STAGE_MINIMUM;
      setMaximumHeight(Math.max(MINIMUM_HEIGHT, shell.clientHeight - toolbar - stage));
    }
  }, [panelOpen]);
  useLayoutEffect(measure, [
    measure,
    chain,
    plugins,
    suggested,
    open,
    pickerOpen,
    openEffectId,
    panelHeight,
  ]);
  // An open panel is shown whole: its document's content height is read
  // back while it is open (it changes as the surface loads, and as the
  // player changes what it shows), and the band follows.
  useEffect(() => {
    if (!panelOpen) return;
    const read = () => {
      const measured = surfaceContentHeight(bandRef.current?.querySelector("iframe") ?? null);
      if (measured === null) return;
      const next = Math.max(PANEL_MINIMUM, measured);
      setPanelHeight((current) => (Math.abs(current - next) > 2 ? next : current));
    };
    read();
    const timer = window.setInterval(read, PANEL_MEASURE_MS);
    return () => window.clearInterval(timer);
  }, [panelOpen, openEffectId]);
  useEffect(() => {
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [measure]);
  useEffect(() => storeHeight(HEIGHT_STORAGE_KEY, chosenHeight), [chosenHeight]);
  useEffect(
    () => storeHeight(PANEL_HEIGHT_STORAGE_KEY, chosenPanelHeight),
    [chosenPanelHeight],
  );

  const clampHeight = (height: number) =>
    Math.min(maximumHeight, Math.max(MINIMUM_HEIGHT, height));
  // With a panel open the drawer has its own chosen height, so the height
  // the player likes for the bare chain never cuts a panel short.
  const chosen = panelOpen ? chosenPanelHeight : chosenHeight;
  const setChosen = panelOpen ? setChosenPanelHeight : setChosenHeight;
  const height = clampHeight(chosen ?? naturalHeight);

  const beginResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    gestureRef.current = { pointerId: event.pointerId, startY: event.clientY, height };
    setResizing(true);
  };
  const resize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    // The grip is on the drawer's lower edge: dragging down grows it.
    setChosen(clampHeight(gesture.height + event.clientY - gesture.startY));
  };
  const finishResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (gestureRef.current?.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    gestureRef.current = null;
    setResizing(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };
  const resizeWithKeyboard = (event: KeyboardEvent<HTMLButtonElement>) => {
    const direction = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (!direction) return;
    event.preventDefault();
    event.stopPropagation();
    setChosen(clampHeight(height + direction * (event.shiftKey ? 40 : 10)));
  };

  const available = effectPlugins(plugins, instances);
  const suggestions = suggestedEffects(suggested, plugins, chain);
  // Offered only when the instrument is recommending more than one thing
  // the player has not already taken: with a single one left, "Add all"
  // is a second button for what the button beside it already does.
  const addableSuggestions = suggestions.filter(isAddableSuggestion);
  const byId = (pluginId: string) => plugins.find((plugin) => plugin.plugin_id === pluginId);
  const shown = phase !== "closed";
  const tab = shown ? 0 : -1;

  return (
    <div
      ref={hostRef}
      className={`play-chain-drawer ${phase}${resizing ? " resizing" : ""}${
        panelOpen ? " with-panel" : ""
      }`}
      style={{ "--play-chain-height": `${height}px` } as CSSProperties}
      onTransitionEnd={(event) => {
        if (event.target === event.currentTarget && event.propertyName === "height" && !open) {
          setSettled(true);
        }
      }}
    >
      <div
        ref={contentRef}
        id="play-chain"
        className="play-chain-panel"
        role="region"
        aria-label="Effects"
        aria-hidden={!shown}
      >
        <div className="play-chain-head">
          <span className="eyebrow accent">Effects</span>
          <span className="play-chain-status">
            Audio path: instrument → every effect that is on, in order → output.
          </span>
          <button
            type="button"
            className="play-chain-close"
            onClick={onClose}
            aria-label="Close effects"
            tabIndex={tab}
          >
            ×
          </button>
        </div>
        <ol className="play-chain-nodes">
          <li className="play-chain-node instrument">
            <PluginIcon
              plugin={instrumentDescriptor}
              name={instrumentName}
              className="play-chain-icon"
            />
            <span className="play-chain-copy">
              <small>Instrument</small>
              <strong>{instrumentName}</strong>
              {instrumentVersion ? <em>{instrumentVersion}</em> : null}
            </span>
          </li>
          {chain.effects.map((effect, index) => {
            const descriptor = byId(effect.plugin_id);
            const name = descriptor?.plugin_name ?? effect.plugin_id;
            const state = effect.enabled ? "" : " bypassed";
            const missing = descriptor ? "" : " missing";
            const opened = openEffectId === effect.id ? " open" : "";
            return (
              <li key={effect.id} className={`play-chain-node effect${state}${missing}${opened}`}>
                <PluginIcon plugin={descriptor} name={name} className="play-chain-icon" />
                <span className="play-chain-copy">
                  <small>{descriptor ? "Effect" : "Not installed"}</small>
                  <strong>{name}</strong>
                </span>
                <span className="play-chain-tools">
                  <button
                    type="button"
                    onClick={() => onChange(withEffectMoved(chain, effect.id, -1))}
                    disabled={index === 0}
                    aria-label={`Move ${name} earlier`}
                    tabIndex={tab}
                  >
                    ◀
                  </button>
                  <button
                    type="button"
                    onClick={() => onChange(withEffectMoved(chain, effect.id, 1))}
                    disabled={index === chain.effects.length - 1}
                    aria-label={`Move ${name} later`}
                    tabIndex={tab}
                  >
                    ▶
                  </button>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={effect.enabled}
                    className={effect.enabled ? "on" : "off"}
                    onClick={() => onChange(withEffectEnabled(chain, effect.id, !effect.enabled))}
                    aria-label={`${name} ${effect.enabled ? "on" : "bypassed"}`}
                    tabIndex={tab}
                  >
                    {effect.enabled ? "On" : "Off"}
                  </button>
                  {descriptor && onOpenEffect ? (
                    <button
                      type="button"
                      className={openEffectId === effect.id ? "on" : ""}
                      onClick={() => onOpenEffect(openEffectId === effect.id ? null : effect.id)}
                      aria-expanded={openEffectId === effect.id}
                      aria-label={`${name} panel`}
                      tabIndex={tab}
                    >
                      Panel
                    </button>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => onChange(withoutEffect(chain, effect.id))}
                    aria-label={`Remove ${name}`}
                    tabIndex={tab}
                  >
                    ×
                  </button>
                </span>
              </li>
            );
          })}
          <li className={`play-chain-node add${pickerOpen ? " active" : ""}`}>
            <button
              type="button"
              className="play-chain-add"
              onClick={() => setPickerOpen((state) => !state)}
              aria-expanded={pickerOpen}
              aria-controls="play-chain-picker"
              tabIndex={tab}
            >
              <span aria-hidden="true">+</span>
              <strong>Add effect</strong>
            </button>
          </li>
          <li className="play-chain-node output">
            <span className="play-chain-copy">
              <small>Output</small>
              <strong>Main</strong>
            </span>
          </li>
        </ol>
        {pickerOpen ? (
          <div id="play-chain-picker" className="play-chain-picker">
            <small>Add</small>
            {available.length === 0 ? (
              <span className="play-chain-picker-empty">No effect plugins installed.</span>
            ) : (
              available.map((plugin) => (
                <button
                  key={plugin.plugin_id}
                  type="button"
                  onClick={() => {
                    setPickerOpen(false);
                    onChange(withEffect(chain, plugin.plugin_id));
                  }}
                  tabIndex={tab}
                >
                  <PluginIcon plugin={plugin} name={plugin.plugin_name} className="play-chain-icon" />
                  <strong>{plugin.plugin_name}</strong>
                  <em>{plugin.version}</em>
                </button>
              ))
            )}
          </div>
        ) : null}
        {effectPanel ? (
          <div ref={bandRef} className="play-chain-effect-panel" style={{ height: panelHeight }}>
            {effectPanel}
          </div>
        ) : null}
        {suggestions.length > 0 ? (
          <div className="play-chain-suggested">
            <small>{instrumentName} suggests</small>
            {addableSuggestions.length > 1 ? (
              <button
                type="button"
                className="play-chain-suggest-all"
                onClick={() => onChange(withSuggestedEffects(chain, suggestions))}
                tabIndex={tab}
              >
                Add all
              </button>
            ) : null}
            {suggestions.map((suggestion) => (
              <span
                key={suggestion.plugin_id}
                className={`play-chain-suggestion${suggestion.descriptor ? "" : " missing"}`}
              >
                <strong>{suggestion.descriptor?.plugin_name ?? suggestion.plugin_id}</strong>
                {suggestion.preset ? <em>{suggestion.preset}</em> : null}
                {suggestion.descriptor ? (
                  suggestion.inChain ? (
                    <i>in chain</i>
                  ) : (
                    <button
                      type="button"
                      onClick={() =>
                        onChange(withEffect(chain, suggestion.plugin_id, suggestion.preset))}
                      tabIndex={tab}
                    >
                      Add
                    </button>
                  )
                ) : (
                  <i>not installed</i>
                )}
              </span>
            ))}
          </div>
        ) : null}
      </div>
      <button
        type="button"
        className="play-chain-grip"
        role="separator"
        aria-label="Resize effects"
        aria-orientation="horizontal"
        aria-valuemin={MINIMUM_HEIGHT}
        aria-valuemax={Number.isFinite(maximumHeight) ? Math.round(maximumHeight) : undefined}
        aria-valuenow={Math.round(height)}
        title="Drag to resize · Double-click to restore automatic height"
        tabIndex={tab}
        onPointerDown={beginResize}
        onPointerMove={resize}
        onPointerUp={finishResize}
        onPointerCancel={finishResize}
        onLostPointerCapture={() => {
          gestureRef.current = null;
          setResizing(false);
        }}
        onDoubleClick={(event) => {
          event.preventDefault();
          setChosen(undefined);
        }}
        onKeyDown={resizeWithKeyboard}
      >
        <span aria-hidden="true" />
      </button>
    </div>
  );
}

/** The grip strip's height in the row; matches `.play-chain-grip`. */
const GRIP_HEIGHT = 14;
