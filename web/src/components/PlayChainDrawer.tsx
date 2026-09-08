import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { PluginInstance, PluginWebDescriptor } from "../types";
import {
  effectPlugins,
  suggestedEffects,
  withEffect,
  withEffectEnabled,
  withEffectMoved,
  withoutEffect,
  type PlayChain,
  type SuggestedChainEntry,
} from "../playChain";
import { PluginIcon } from "./PluginIcon";

const HEIGHT_STORAGE_KEY = "rackforge.play.chain-height.v1";
/** The head and the grip alone. */
const MINIMUM_HEIGHT = 64;
/** What the plugin keeps below the drawer at the drawer's tallest. */
const STAGE_MINIMUM = 160;
/** A close whose `transitionend` never came (a hidden tab, a 0 ms motion) still settles. */
const SETTLE_FALLBACK_MS = 600;

function storedHeight(): number | undefined {
  try {
    const stored = JSON.parse(window.localStorage.getItem(HEIGHT_STORAGE_KEY) ?? "null");
    return typeof stored === "number" && Number.isFinite(stored) ? stored : undefined;
  } catch {
    return undefined;
  }
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
  const [chosenHeight, setChosenHeight] = useState<number | undefined>(storedHeight);
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
  const measure = useCallback(() => {
    const content = contentRef.current;
    const host = hostRef.current;
    if (content) setNaturalHeight(Math.max(MINIMUM_HEIGHT, content.scrollHeight + GRIP_HEIGHT));
    const shell = host?.parentElement;
    if (shell) {
      setMaximumHeight(Math.max(MINIMUM_HEIGHT, shell.clientHeight - STAGE_MINIMUM));
    }
  }, []);
  useLayoutEffect(measure, [measure, chain, plugins, suggested, open, pickerOpen]);
  useEffect(() => {
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [measure]);
  useEffect(() => {
    try {
      if (chosenHeight === undefined) window.localStorage.removeItem(HEIGHT_STORAGE_KEY);
      else window.localStorage.setItem(HEIGHT_STORAGE_KEY, JSON.stringify(chosenHeight));
    } catch {
      // Persistent sizing is optional in hardened or ephemeral WebViews.
    }
  }, [chosenHeight]);

  const clampHeight = (height: number) =>
    Math.min(maximumHeight, Math.max(MINIMUM_HEIGHT, height));
  const height = clampHeight(chosenHeight ?? naturalHeight);

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
    setChosenHeight(clampHeight(gesture.height + event.clientY - gesture.startY));
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
    setChosenHeight(clampHeight(height + direction * (event.shiftKey ? 40 : 10)));
  };

  const available = effectPlugins(plugins, instances);
  const suggestions = suggestedEffects(suggested, plugins, chain);
  const byId = (pluginId: string) => plugins.find((plugin) => plugin.plugin_id === pluginId);
  const shown = phase !== "closed";
  const tab = shown ? 0 : -1;

  return (
    <div
      ref={hostRef}
      className={`play-chain-drawer ${phase}${resizing ? " resizing" : ""}`}
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
            return (
              <li key={effect.id} className={`play-chain-node effect${state}${missing}`}>
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
        {suggestions.length > 0 ? (
          <div className="play-chain-suggested">
            <small>{instrumentName} suggests</small>
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
                      onClick={() => onChange(withEffect(chain, suggestion.plugin_id))}
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
          setChosenHeight(undefined);
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
