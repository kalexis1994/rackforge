import {
  Background,
  BackgroundVariant,
  BaseEdge,
  ControlButton,
  Controls,
  EdgeLabelRenderer,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  applyNodeChanges,
  getBezierPath,
  useConnection,
  type Connection,
  type Edge,
  type EdgeProps,
  type EdgeTypes,
  type FinalConnectionState,
  type Node,
  type NodeChange,
  type NodeProps,
  type NodeTypes,
  type ReactFlowInstance,
  type Viewport,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { History, Redo2, Undo2 } from "lucide-react";
import type { DraftHistory } from "../hooks/useDraftHistory";
import { pluginKind, usePluginCatalog } from "../pluginCatalog";
import { pluginKindPresentation } from "../pluginPresentation";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import {
  connectRackGraph,
  materializeRackGraph,
  midiTransformFromSlot,
  normalizeRackGraphPosition,
  rackConnectionProblem,
  rackGraphId,
  rackGraphProblems,
  removeSlotFromRack,
  type RackConnection,
} from "../rackGraph";
import type { RackPluginRole } from "../rackPluginSelection";
import { RackSlotPopover } from "./RackSlotPopover";
import { RackMidiLinkEditor } from "./RackMidiLinkEditor";
import type {
  PluginInstance,
  PluginStateReference,
  RackDefinition,
  RackGraphEdge,
  RackGraphLabel,
  RackGraphLabelTone,
  RackGraphNode,
  RackGraphPosition,
  RackGraphSignal,
} from "../types";

type CanvasNodeData = {
  title: string;
  subtitle: string;
  kind: RackGraphNode["kind"]["kind"] | "label";
  /** What a plugin node's plugin is -- "instrument", "effect" or
   *  "midi-processor" -- so it takes its kind's colour. */
  pluginKind?: string;
  /** The plugin's own artwork, when it has any: its banner behind the node
   *  and its icon in place of the generic mark. */
  bannerUrl?: string;
  iconUrl?: string;
  /** The worst problem this node has as it is wired; see rackGraphProblems. */
  problem?: "error" | "warning";
  labelKind?: RackGraphLabel["kind"];
  tone?: RackGraphLabelTone;
};

type CanvasNode = Node<CanvasNodeData>;

type RackCanvasEdgeData = {
  signal: RackGraphSignal;
  editable: boolean;
  onOpen: (edgeId: string, clientX: number, clientY: number) => void;
  onSelect: (edgeId: string) => void;
};

type RackCanvasEdge = Edge<RackCanvasEdgeData, "rackFlow">;

type GraphMenuAnchor = {
  position: RackGraphPosition;
  offset: RackGraphPosition;
};

type CanvasBounds = {
  left: number;
  top: number;
  width: number;
  height: number;
};

const RackNodeCard = memo(function RackNodeCard({ data, selected }: NodeProps<CanvasNode>) {
  const acceptsMidi = data.kind === "plugin" || data.kind === "rack";
  const emitsAudio = acceptsMidi;
  const acceptsAudio = data.kind === "plugin" || data.kind === "rack";
  return (
    <div className={`rack-flow-node ${data.kind}${data.pluginKind ? ` ${data.pluginKind}` : ""}${data.bannerUrl ? " branded" : ""}${data.problem ? ` has-${data.problem}` : ""} ${selected ? "selected" : ""}`}>
      {data.bannerUrl ? (
        // Clipped on its own: the node itself cannot hide its overflow, the
        // ports sit half outside its edge.
        <span className="rack-flow-node-art" aria-hidden="true">
          <img src={data.bannerUrl} alt="" draggable={false} />
        </span>
      ) : null}
      {acceptsMidi ? (
        <Handle
          id="midi:midi_in"
          type="target"
          position={Position.Left}
          className="midi-handle rack-midi-input-handle"
        />
      ) : null}
      {data.kind === "audio_output" ? (
        <Handle
          id="audio:in"
          type="target"
          position={Position.Left}
          className="audio-handle"
        />
      ) : null}
      {acceptsAudio ? (
        <Handle
          id="audio:audio_in"
          type="target"
          position={Position.Left}
          className="audio-handle rack-audio-input-handle"
        />
      ) : null}
      <span className={`rack-flow-node-icon${data.iconUrl ? " has-art" : ""}`}>
        {data.iconUrl ? (
          <img src={data.iconUrl} alt="" draggable={false} />
        ) : data.kind === "midi_input"
          ? "MIDI"
          : data.kind === "audio_input"
            ? "IN"
          : data.kind === "audio_output"
            ? "OUT"
            : data.kind === "rack"
              ? "RACK"
              : "RF"}
      </span>
      <div>
        <strong>{data.title}</strong>
        {data.subtitle ? <small>{data.subtitle}</small> : null}
      </div>
      {data.kind === "midi_input" ? (
        <Handle
          id="midi:out"
          type="source"
          position={Position.Right}
          className="midi-handle"
        />
      ) : null}
      {data.kind === "audio_input" ? (
        <Handle
          id="audio:out"
          type="source"
          position={Position.Right}
          className="audio-handle"
        />
      ) : null}
      {emitsAudio ? (
        <Handle
          id="audio:audio_out"
          type="source"
          position={Position.Right}
          className="audio-handle"
        />
      ) : null}
    </div>
  );
});

const LabelCard = memo(function LabelCard({ data, selected }: NodeProps<CanvasNode>) {
  return (
    <div
      className={`rack-flow-label ${data.labelKind ?? "note"} tone-${data.tone ?? "neutral"} ${selected ? "selected" : ""}`}
    >
      <span>{data.labelKind === "section" ? "SECTION" : "NOTE"}</span>
      <strong>{data.title}</strong>
    </div>
  );
});

const nodeTypes: NodeTypes = {
  rackNode: RackNodeCard,
  labelNode: LabelCard,
};

// The colour code's lit set (design/tokens.css): the canvas is dark in both
// lighting modes. A plugin is its kind; a rack, a container, is grey; the
// ins and outs are their signal, MIDI violet and audio amber.
const graphNodeColors: Record<CanvasNodeData["kind"], string> = {
  plugin: "var(--rf-lit-instrument)",
  rack: "var(--rf-lit-rack)",
  midi_input: "var(--rf-lit-input)",
  audio_input: "var(--rf-lit-sound)",
  midi_output: "var(--rf-lit-input)",
  audio_output: "var(--rf-lit-sound)",
  label: "#70838e",
};

const MINIMAP_WIDTH = 160;
const MINIMAP_HEIGHT = 100;

const graphPluginColors: Record<string, string> = {
  effect: "var(--rf-lit-sound)",
  "midi-processor": "var(--rf-lit-input)",
};

function graphMiniMapNodeColor(node: CanvasNode) {
  return (node.data.pluginKind && graphPluginColors[node.data.pluginKind])
    || graphNodeColors[node.data.kind];
}

const RackFlowEdge = memo(function RackFlowEdge({
  id,
  source,
  sourceHandleId,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  markerEnd,
  style,
  selected,
  data,
}: EdgeProps<RackCanvasEdge>) {
  const [path, controlX, controlY] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  });
  // An audio output feeds one place, so a cable drawn from one already
  // patched takes this one's place: while it is drawn, this one shows it is
  // about to go. The selector answers yes or no, so a drag re-renders an
  // edge only when that answer changes, not on every pointer move.
  const replacing = useConnection((connection) =>
    connection.inProgress
    && data?.signal === "audio"
    && connection.fromHandle.type === "source"
    && connection.fromNode.id === source
    && connection.fromHandle.id === sourceHandleId);
  return (
    <>
      <BaseEdge
        id={id}
        path={path}
        markerEnd={markerEnd}
        style={style}
        className={`rack-flow-edge-path ${data?.signal ?? "audio"}${replacing ? " is-replaced" : ""}`}
      />
      {data?.editable ? (
        <EdgeLabelRenderer>
          <button
            type="button"
            className={`rack-edge-control nodrag nopan${selected ? " selected" : ""}`}
            style={{ transform: `translate(-50%, -50%) translate(${controlX}px, ${controlY}px)` }}
            data-edge-id={id}
            aria-label="MIDI connection settings"
            onClick={(event) => {
              event.stopPropagation();
              data.onSelect(id);
            }}
            onDoubleClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              data.onOpen(id, event.clientX, event.clientY);
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter" && event.key !== " ") return;
              event.preventDefault();
              const bounds = event.currentTarget.getBoundingClientRect();
              data.onOpen(id, bounds.left + bounds.width / 2, bounds.top + bounds.height / 2);
            }}
          >
            <span />
          </button>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
});

const edgeTypes: EdgeTypes = {
  rackFlow: RackFlowEdge,
};

const labelTones: RackGraphLabelTone[] = [
  "neutral",
  "cyan",
  "green",
  "amber",
  "violet",
  "red",
];

function nodeTitle(node: RackGraphNode, rack: RackDefinition, racks: RackDefinition[]) {
  switch (node.kind.kind) {
    case "midi_input":
      return ["MIDI Input", node.kind.bus_id];
    case "audio_input":
      return ["Audio Input", node.kind.bus_id];
    case "audio_output":
      return ["Audio Output", node.kind.bus_id];
    case "midi_output":
      return ["MIDI Output", node.kind.bus_id];
    case "plugin": {
      const slotId = node.kind.slot_id;
      const slot = rack.slots.find((candidate) => candidate.id === slotId);
      // The plugin's name alone: its id says nothing on a stage.
      return [slot?.name ?? "Missing Slot", slot ? "" : slotId];
    }
    case "rack": {
      const rackId = node.kind.rack_id;
      const child = racks.find((candidate) => candidate.id === rackId);
      return [child?.name ?? "Missing Rack", "Child Rack"];
    }
  }
}

function toCanvasNodes(
  rack: RackDefinition,
  racks: RackDefinition[],
  pluginKinds: ReadonlyMap<string, string>,
  pluginArt: ReadonlyMap<string, { bannerUrl?: string; iconUrl?: string; version?: string }> = new Map(),
): CanvasNode[] {
  const graph = materializeRackGraph(rack).graph!;
  const nodes = graph.nodes.map((node): CanvasNode => {
    const [title, subtitle] = nodeTitle(node, rack, racks);
    const slotId = node.kind.kind === "plugin" ? node.kind.slot_id : null;
    const pluginId = slotId === null
      ? null
      : rack.slots.find((candidate) => candidate.id === slotId)?.plugin_id ?? null;
    const kind = pluginId === null ? undefined : pluginKinds.get(pluginId);
    return {
      id: node.id,
      type: "rackNode",
      position: node.position,
      data: {
        title,
        // A plugin node's small line is its version, in the catalog's words.
        subtitle: (pluginId && pluginArt.get(pluginId)?.version) || subtitle,
        kind: node.kind.kind,
        ...(kind ? { pluginKind: kind } : {}),
        ...(pluginId ? {
          bannerUrl: pluginArt.get(pluginId)?.bannerUrl,
          iconUrl: pluginArt.get(pluginId)?.iconUrl,
        } : {}),
      },
      deletable: false,
    };
  });
  for (const label of graph.labels ?? []) {
    nodes.push({
      id: `label:${label.id}`,
      type: "labelNode",
      position: label.position,
      data: {
        title: label.text,
        subtitle: "",
        kind: "label",
        labelKind: label.kind,
        tone: label.tone,
      },
      style: { width: label.width, height: label.height },
      zIndex: label.kind === "section" ? -10 : 10,
      deletable: true,
    });
  }
  return nodes;
}

function toCanvasEdges(
  edges: RackGraphEdge[],
  nodes: RackGraphNode[],
  selectedId: string | undefined,
  onOpen: RackCanvasEdgeData["onOpen"],
  onSelect: RackCanvasEdgeData["onSelect"],
): RackCanvasEdge[] {
  const nodeKinds = new Map(nodes.map((node) => [node.id, node.kind.kind]));
  return edges.map((edge) => ({
    id: edge.id,
    type: "rackFlow",
    source: edge.source.node_id,
    target: edge.target.node_id,
    sourceHandle: `${edge.signal}:${edge.source.port_id}`,
    targetHandle: `${edge.signal}:${edge.target.port_id}`,
    className: `rack-flow-edge ${edge.signal}`,
    selected: selectedId === edge.id,
    data: {
      signal: edge.signal,
      editable: edge.signal === "midi"
        && nodeKinds.get(edge.source.node_id) === "midi_input"
        && ["plugin", "rack"].includes(nodeKinds.get(edge.target.node_id) ?? ""),
      onOpen,
      onSelect,
    },
    // MIDI is drawn dashed (faceplate.css), but still: running dashes repaint
    // the canvas every frame, which a Raspberry Pi pays for the whole time a
    // Rack is open.
    markerEnd: { type: MarkerType.ArrowClosed },
    style: {
      // The signal's colour in the code's lit set: MIDI violet, audio amber.
      stroke: edge.signal === "midi" ? "var(--rf-lit-input)" : "var(--rf-lit-sound)",
      strokeWidth: 2,
    },
  }));
}

/** A React Flow connection as the graph's rules read one, or null when its
 *  handles do not name a signal and a port. */
function toRackConnection(connection: {
  source: string | null;
  target: string | null;
  sourceHandle?: string | null;
  targetHandle?: string | null;
}): RackConnection | null {
  const source = decodeHandle(connection.sourceHandle);
  const target = decodeHandle(connection.targetHandle);
  if (!connection.source || !connection.target || !source || !target) return null;
  if (source.signal !== target.signal) return null;
  return {
    signal: source.signal,
    source: { node_id: connection.source, port_id: source.portId },
    target: { node_id: connection.target, port_id: target.portId },
  };
}

function decodeHandle(handle: string | null | undefined) {
  if (!handle) return undefined;
  const separator = handle.indexOf(":");
  if (separator < 1) return undefined;
  return {
    signal: handle.slice(0, separator) as RackGraphSignal,
    portId: handle.slice(separator + 1),
  };
}

function dependsOn(
  sourceId: string,
  targetId: string,
  racks: Map<string, RackDefinition>,
  visited = new Set<string>(),
): boolean {
  if (sourceId === targetId) return true;
  if (visited.has(sourceId)) return false;
  visited.add(sourceId);
  const source = racks.get(sourceId);
  if (!source?.graph) return false;
  return source.graph.nodes.some(
    (node) =>
      node.kind.kind === "rack" &&
      dependsOn(node.kind.rack_id, targetId, racks, visited),
  );
}

interface RackGraphEditorProps {
  rack: RackDefinition;
  racks: RackDefinition[];
  /** The editing history of the draft this graph belongs to. */
  history?: DraftHistory;
  onChange: (
    update: RackDefinition | ((current: RackDefinition) => RackDefinition),
  ) => void;
  canAddInstrument: boolean;
  onAddInstrument: (position: RackGraphPosition, role: RackPluginRole) => void;
  instances: PluginInstance[];
  renderPluginSurface: (options: {
    instance: PluginInstance;
    state?: PluginStateReference;
    onStateChange: (state: PluginStateReference) => void;
    onSelectSound: (soundId: string) => Promise<unknown>;
    parameterLinkInstanceId: string;
  }) => ReactNode;
  onOverlayChange?: (open: boolean) => void;
}

export default function RackGraphEditor({
  rack,
  racks,
  history,
  onChange,
  canAddInstrument,
  onAddInstrument,
  instances,
  renderPluginSurface,
  onOverlayChange,
}: RackGraphEditorProps) {
  const materialized = useMemo(() => materializeRackGraph(rack), [rack]);
  const [selectedId, setSelectedId] = useState<string>();
  const [editorSlotId, setEditorSlotId] = useState<string>();
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const suppressPaneClickRef = useRef(false);
  const [viewport, setViewport] = useState<Viewport>({ x: 0, y: 0, zoom: 1 });
  const [canvasBounds, setCanvasBounds] = useState<CanvasBounds>({
    left: 0,
    top: 0,
    width: 0,
    height: 0,
  });
  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const measure = () => {
      const bounds = canvas.getBoundingClientRect();
      setCanvasBounds({
        left: bounds.left,
        top: bounds.top,
        width: bounds.width,
        height: bounds.height,
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(canvas);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);
  const paneGestureRef = useRef<{
    pointerId: number;
    x: number;
    y: number;
    timer: number;
    nodeId?: string;
    edgeId?: string;
  } | null>(null);
  const [nodeMenu, setNodeMenu] = useState<{
    nodeId: string;
    anchor: GraphMenuAnchor;
  } | null>(null);
  const [paneMenu, setPaneMenu] = useState<GraphMenuAnchor | null>(null);
  const [midiLinkEditor, setMidiLinkEditor] = useState<{
    edgeId: string;
    anchor: GraphMenuAnchor;
  } | null>(null);
  const overlayOpen = editorSlotId !== undefined || midiLinkEditor !== null;
  const [historyOpen, setHistoryOpen] = useState(false);
  // Undo and redo from the keyboard, as everywhere: Ctrl/Cmd+Z, and
  // Ctrl/Cmd+Shift+Z or Ctrl+Y. A text field keeps its own undo.
  useEffect(() => {
    if (!history) return;
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [contenteditable='true']")) return;
      const key = event.key.toLowerCase();
      if (key === "z" && !event.shiftKey) {
        event.preventDefault();
        history.undo();
      } else if ((key === "z" && event.shiftKey) || key === "y") {
        event.preventDefault();
        history.redo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [history]);
  // A finger needs a wider reach than a pointer to land a cable on a port.
  const coarsePointer = useMemo(
    () => typeof window !== "undefined" && window.matchMedia("(pointer: coarse)").matches,
    [],
  );
  // A node added to the Rack is brought into view with the rest, at the
  // emphasized step, so an effect inserted past the edge of the canvas -- or
  // the output stepping right for it -- is not left off screen. Only when the
  // Rack gains a node: opening it fits on its own, and moving or removing
  // leaves the view where the player put it.
  const flowRef = useRef<ReactFlowInstance<CanvasNode, RackCanvasEdge> | null>(null);
  const graphNodeCount = materialized.graph!.nodes.length;
  const seenNodeCountRef = useRef(graphNodeCount);
  useEffect(() => {
    const grew = graphNodeCount > seenNodeCountRef.current;
    seenNodeCountRef.current = graphNodeCount;
    if (!grew || !flowRef.current) return;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const frame = window.requestAnimationFrame(() => {
      void flowRef.current?.fitView({
        padding: 0.18,
        maxZoom: 1.15,
        duration: still ? 0 : 320,
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [graphNodeCount]);
  useEffect(() => {
    onOverlayChange?.(overlayOpen);
  }, [onOverlayChange, overlayOpen]);
  useEffect(() => () => onOverlayChange?.(false), [onOverlayChange]);
  const createMenuAnchor = useCallback(
    (clientX: number, clientY: number, width: number, height: number) => {
      if (!canvasBounds.width || !canvasBounds.height) return null;
      const gap = 8;
      const localX = clientX - canvasBounds.left;
      const localY = clientY - canvasBounds.top;
      const position = normalizeRackGraphPosition({
        x: (localX - viewport.x) / viewport.zoom,
        y: (localY - viewport.y) / viewport.zoom,
      });
      const x = Math.max(gap, Math.min(canvasBounds.width - width - gap, localX + gap));
      const y = Math.max(gap, Math.min(canvasBounds.height - height - gap, localY + gap));
      return {
        position,
        offset: { x: x - localX, y: y - localY },
      };
    },
    [canvasBounds, viewport],
  );
  const menuStyle = useCallback(
    (anchor: GraphMenuAnchor) => {
      return {
        left: viewport.x + anchor.position.x * viewport.zoom + anchor.offset.x,
        top: viewport.y + anchor.position.y * viewport.zoom + anchor.offset.y,
      };
    },
    [viewport],
  );
  const openNodeMenu = useCallback((
    nodeId: string,
    clientX: number,
    clientY: number,
  ) => {
    const anchor = createMenuAnchor(clientX, clientY, 182, 170);
    if (!anchor) return;
    setSelectedId(nodeId);
    setPaneMenu(null);
    setMidiLinkEditor(null);
    setNodeMenu({ nodeId, anchor });
  }, [createMenuAnchor]);
  const openPaneMenu = useCallback((clientX: number, clientY: number) => {
    const anchor = createMenuAnchor(clientX, clientY, 218, 286);
    if (!anchor) return;
    setSelectedId(undefined);
    setNodeMenu(null);
    setMidiLinkEditor(null);
    setPaneMenu(anchor);
  }, [createMenuAnchor]);
  const openMidiLinkEditor = useCallback((edgeId: string, clientX: number, clientY: number) => {
    const edge = materialized.graph!.edges.find((candidate) => candidate.id === edgeId);
    const source = edge
      ? materialized.graph!.nodes.find((node) => node.id === edge.source.node_id)
      : undefined;
    const target = edge
      ? materialized.graph!.nodes.find((node) => node.id === edge.target.node_id)
      : undefined;
    if (
      !edge
      || edge.signal !== "midi"
      || source?.kind.kind !== "midi_input"
      || !target
      || !["plugin", "rack"].includes(target.kind.kind)
    ) return;
    const anchor = createMenuAnchor(clientX, clientY, 486, 650);
    if (!anchor) return;
    setSelectedId(edgeId);
    setNodeMenu(null);
    setPaneMenu(null);
    setMidiLinkEditor({ edgeId, anchor });
  }, [createMenuAnchor, materialized]);
  const finishPaneGesture = useCallback(() => {
    const gesture = paneGestureRef.current;
    if (!gesture) return;
    window.clearTimeout(gesture.timer);
    paneGestureRef.current = null;
  }, []);
  const beginPaneGesture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const origin = event.target instanceof Element ? event.target : null;
      const nodeElement = origin?.closest<HTMLElement>(".react-flow__node");
      const edgeControl = origin?.closest<HTMLElement>(".rack-edge-control");
      const isPane = origin?.classList.contains("react-flow__pane");
      if (
        !event.isPrimary ||
        event.button !== 0 ||
        (!isPane && !nodeElement && !edgeControl)
      ) {
        return;
      }
      finishPaneGesture();
      const { clientX: x, clientY: y, pointerId } = event;
      paneGestureRef.current = {
        pointerId,
        x,
        y,
        nodeId: nodeElement?.dataset.id,
        edgeId: edgeControl?.dataset.edgeId,
        timer: window.setTimeout(() => {
          if (paneGestureRef.current?.pointerId !== pointerId) return;
          const gesture = paneGestureRef.current;
          paneGestureRef.current = null;
          suppressPaneClickRef.current = true;
          if (gesture.nodeId) {
            openNodeMenu(gesture.nodeId, x, y);
          } else if (gesture.edgeId) {
            openMidiLinkEditor(gesture.edgeId, x, y);
          } else {
            openPaneMenu(x, y);
          }
        }, 520),
      };
    },
    [finishPaneGesture, openMidiLinkEditor, openNodeMenu, openPaneMenu],
  );
  const movePaneGesture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const gesture = paneGestureRef.current;
      if (
        gesture?.pointerId === event.pointerId &&
        Math.hypot(event.clientX - gesture.x, event.clientY - gesture.y) > 20
      ) {
        finishPaneGesture();
      }
    },
    [finishPaneGesture],
  );
  const { plugins: catalogPlugins } = usePluginCatalog();
  const pluginArt = useMemo(
    () => new Map(catalogPlugins.map((plugin) => [
      plugin.plugin_id,
      {
        ...(plugin.branding
          ? { bannerUrl: plugin.branding.banner_url, iconUrl: plugin.branding.icon_url }
          : {}),
        ...(plugin.version ? { version: `v${plugin.version.replace(/^v/i, "")}` } : {}),
      },
    ])),
    [catalogPlugins],
  );
  const pluginKinds = useMemo(
    () => new Map(catalogPlugins.map((plugin) => [
      plugin.plugin_id,
      pluginKindPresentation(pluginKind(plugin)).className,
    ])),
    [catalogPlugins],
  );
  const problems = useMemo(
    () => rackGraphProblems(materialized.graph!, {
      slots: materialized.slots,
      slotRole: (slot) => {
        const kind = pluginKinds.get(slot.plugin_id);
        return kind === "instrument" || kind === "effect" ? kind : undefined;
      },
    }),
    [materialized, pluginKinds],
  );
  const errorCount = problems.filter((problem) => problem.severity === "error").length;
  const mappedNodes = useMemo(() => {
    // Problems come worst first, so the first one found for a node is its mark.
    const worst = new Map<string, "error" | "warning">();
    for (const problem of problems) {
      if (!worst.has(problem.nodeId)) worst.set(problem.nodeId, problem.severity);
    }
    return toCanvasNodes(materialized, racks, pluginKinds, pluginArt).map((node) => {
      const problem = worst.get(node.id);
      return problem ? { ...node, data: { ...node.data, problem } } : node;
    });
  }, [materialized, racks, pluginKinds, pluginArt, problems]);
  const [interactiveNodes, setInteractiveNodes] = useState(mappedNodes);
  const interactiveRackIdRef = useRef(rack.id);
  useLayoutEffect(() => {
    const changedRack = interactiveRackIdRef.current !== rack.id;
    interactiveRackIdRef.current = rack.id;
    setInteractiveNodes((current) => {
      if (changedRack) return mappedNodes;

      // React Flow enriches controlled nodes with measured dimensions and
      // transient interaction state. Preview/session refreshes can recreate
      // the Rack prop without changing its graph; replacing every node with
      // the raw mapped version makes Android WebView discard and rebuild the
      // complete transformed layer. Reconcile by id so those refreshes cannot
      // make a valid persisted graph flash blank or remain visually empty.
      const currentById = new Map(current.map((node) => [node.id, node]));
      return mappedNodes.map((node) => {
        const existing = currentById.get(node.id);
        if (!existing) return node;
        return {
          ...node,
          selected: existing.selected,
          dragging: existing.dragging,
          measured: existing.measured,
          width: existing.width,
          height: existing.height,
          position: existing.dragging ? existing.position : node.position,
        };
      });
    });
  }, [mappedNodes, rack.id]);
  const edges = useMemo(
    () => toCanvasEdges(
      materialized.graph!.edges,
      materialized.graph!.nodes,
      selectedId,
      openMidiLinkEditor,
      setSelectedId,
    ),
    [materialized.graph, openMidiLinkEditor, selectedId],
  );
  const rackMap = useMemo(() => new Map(racks.map((item) => [item.id, item])), [racks]);
  const childOptions = useMemo(
    () =>
      racks.filter(
        (candidate) =>
          candidate.id !== rack.id && !dependsOn(candidate.id, rack.id, rackMap),
      ),
    [rack.id, rackMap, racks],
  );
  const [childRackId, setChildRackId] = useState("");
  const activeChildRackId = childOptions.some(
    (candidate) => candidate.id === childRackId,
  )
    ? childRackId
    : (childOptions[0]?.id ?? "");

  const updateGraph = useCallback((
    update: (
      graph: NonNullable<RackDefinition["graph"]>,
    ) => NonNullable<RackDefinition["graph"]>,
  ) => {
    onChange((currentRack) => {
      const current = materializeRackGraph(currentRack);
      return { ...current, graph: update(current.graph!) };
    });
  }, [onChange]);
  const updateSlot = useCallback((nextSlot: RackDefinition["slots"][number]) => {
    onChange((currentRack) => {
      const current = materializeRackGraph(currentRack);
      return {
        ...current,
        slots: current.slots.map((slot) =>
          slot.id === nextSlot.id ? nextSlot : slot,
        ),
      };
    });
  }, [onChange]);

  const handleNodesChange = useCallback(
    (changes: NodeChange<CanvasNode>[]) => {
      setInteractiveNodes((current) => applyNodeChanges(
        changes.filter((change) => {
          if (change.type !== "remove") return true;
          return current.some(
            (node) => node.id === change.id && node.data.kind === "label",
          );
        }),
        current,
      ));
    },
    [],
  );

  const persistNodePosition = useCallback(
    (nodeId: string, position: RackGraphPosition) => {
      const normalized = normalizeRackGraphPosition(position);
      updateGraph((graph) => ({
        ...graph,
        nodes: graph.nodes.map((node) =>
          node.id === nodeId ? { ...node, position: normalized } : node,
        ),
        labels: (graph.labels ?? []).map((label) =>
          `label:${label.id}` === nodeId
            ? { ...label, position: normalized }
            : label,
        ),
      }));
    },
    [updateGraph],
  );

  // The graph's rules (rackGraph.ts) decide both: a port that would refuse
  // the cable does not light while it is dragged, and a dropped cable that
  // re-patches an audio output replaces the one it had.
  const isValidConnection = useCallback(
    (connection: Connection | Edge) => {
      const candidate = toRackConnection(connection);
      return candidate !== null
        && rackConnectionProblem(materialized.graph!, candidate) === null;
    },
    [materialized],
  );
  const connect = useCallback(
    (connection: Connection) => {
      const candidate = toRackConnection(connection);
      if (!candidate) return;
      updateGraph((graph) => connectRackGraph(graph, candidate));
    },
    [updateGraph],
  );
  // A cable's end can be picked up and moved to another port, as a plug is,
  // or dropped on empty space to unplug it. Dropped on a port that refuses it,
  // it stays where it was, and says why (explainRefusal).
  const reconnected = useRef(false);
  const reconnect = useCallback(
    (oldEdge: RackCanvasEdge, connection: Connection) => {
      const candidate = toRackConnection(connection);
      if (!candidate) return;
      reconnected.current = true;
      updateGraph((graph) => {
        const previous = graph.edges.find((edge) => edge.id === oldEdge.id);
        if (!previous) return graph;
        const without = { ...graph, edges: graph.edges.filter((edge) => edge !== previous) };
        const next = connectRackGraph(without, candidate);
        if (next === without) return graph;
        // A MIDI cable keeps its routing settings wherever it is plugged.
        return previous.midi_transform
          ? {
            ...next,
            edges: next.edges.map((edge, index) =>
              index === next.edges.length - 1
                ? { ...edge, midi_transform: previous.midi_transform }
                : edge),
          }
          : next;
      });
    },
    [updateGraph],
  );
  const beginReconnect = useCallback(() => {
    reconnected.current = false;
  }, []);
  // A cable let go over a port that refused it says why, briefly, instead of
  // simply not appearing.
  const [refusal, setRefusal] = useState<{ id: number; reason: string } | null>(null);
  useEffect(() => {
    if (!refusal) return;
    const timer = window.setTimeout(() => setRefusal(null), 3_200);
    return () => window.clearTimeout(timer);
  }, [refusal]);
  const explainRefusal = useCallback(
    (_event: MouseEvent | TouchEvent, state: FinalConnectionState) => {
      if (state.isValid !== false || !state.fromHandle || !state.toHandle) return;
      const [from, to] = state.fromHandle.type === "source"
        ? [state.fromHandle, state.toHandle]
        : [state.toHandle, state.fromHandle];
      if (from.type === to.type) return;
      const candidate = toRackConnection({
        source: from.nodeId,
        target: to.nodeId,
        sourceHandle: from.id,
        targetHandle: to.id,
      });
      const reason = candidate
        ? rackConnectionProblem(materialized.graph!, candidate)
        : "MIDI and audio ports do not connect to each other.";
      if (reason) setRefusal({ id: Date.now(), reason });
    },
    [materialized],
  );

  const removeEdges = useCallback(
    (removed: Edge[]) => {
      const removedIds = new Set(removed.map((edge) => edge.id));
      updateGraph((graph) => ({
        ...graph,
        edges: graph.edges.filter((edge) => !removedIds.has(edge.id)),
      }));
      if (midiLinkEditor && removedIds.has(midiLinkEditor.edgeId)) {
        setMidiLinkEditor(null);
      }
    },
    [midiLinkEditor, updateGraph],
  );

  const addLabel = useCallback(
    (kind: RackGraphLabel["kind"], position: RackGraphPosition) => {
      const label: RackGraphLabel = {
        id: rackGraphId("label"),
        text: kind === "section" ? "New section" : "New note",
        kind,
        tone: kind === "section" ? "cyan" : "neutral",
        position,
        width: kind === "section" ? 640 : 240,
        height: kind === "section" ? 320 : 100,
      };
      updateGraph((graph) => ({
        ...graph,
        labels: [...(graph.labels ?? []), label],
      }));
      setSelectedId(`label:${label.id}`);
    },
    [updateGraph],
  );
  const addAudioInput = useCallback((position: RackGraphPosition) => {
    updateGraph((graph) => {
      if (graph.nodes.some(
        (node) => node.kind.kind === "audio_input" && node.kind.bus_id === "main",
      )) return graph;
      return {
        ...graph,
        nodes: [
          ...graph.nodes,
          {
            id: rackGraphId("input.audio"),
            kind: { kind: "audio_input", bus_id: "main" },
            position: normalizeRackGraphPosition(position),
          },
        ],
      };
    });
  }, [updateGraph]);

  const addChildRack = useCallback((position: RackGraphPosition) => {
    if (!activeChildRackId) return;
    const nodeId = rackGraphId("rack");
    updateGraph((graph) => {
      const midiInput = graph.nodes.find((node) => node.kind.kind === "midi_input");
      const audioOutput = graph.nodes.find(
        (node) => node.kind.kind === "audio_output" && node.kind.bus_id === "main",
      );
      if (!midiInput || !audioOutput) return graph;
      return {
        ...graph,
        nodes: [
          ...graph.nodes,
          {
            id: nodeId,
            kind: { kind: "rack", rack_id: activeChildRackId },
            position,
          },
        ],
        edges: [
          ...graph.edges,
          {
            id: rackGraphId("edge.midi"),
            signal: "midi",
            source: { node_id: midiInput.id, port_id: "out" },
            target: { node_id: nodeId, port_id: "midi_in" },
          },
          {
            id: rackGraphId("edge.audio"),
            signal: "audio",
            source: { node_id: nodeId, port_id: "audio_out" },
            target: { node_id: audioOutput.id, port_id: "in" },
          },
        ],
      };
    });
  }, [activeChildRackId, updateGraph]);

  const removeSelected = useCallback(() => {
    if (!selectedId) return;
    if (selectedId.startsWith("label:")) {
      const labelId = selectedId.slice("label:".length);
      updateGraph((graph) => ({
        ...graph,
        labels: (graph.labels ?? []).filter((label) => label.id !== labelId),
      }));
      setSelectedId(undefined);
      return;
    }
    const node = materialized.graph!.nodes.find((candidate) => candidate.id === selectedId);
    if (!node || node.kind.kind === "midi_input" || node.kind.kind.endsWith("output")) return;
    if (node.kind.kind === "plugin") {
      const slotId = node.kind.slot_id;
      onChange((current) => removeSlotFromRack(current, slotId));
    } else {
      updateGraph((graph) => ({
        ...graph,
        nodes: graph.nodes.filter((candidate) => candidate.id !== selectedId),
        edges: graph.edges.filter(
          (edge) =>
            edge.source.node_id !== selectedId && edge.target.node_id !== selectedId,
        ),
      }));
    }
    setSelectedId(undefined);
  }, [materialized, onChange, selectedId, setSelectedId, updateGraph]);

  const selectedLabel = selectedId?.startsWith("label:")
    ? materialized.graph!.labels?.find(
        (label) => label.id === selectedId.slice("label:".length),
      )
    : undefined;
  const menuNode = nodeMenu
    ? materialized.graph!.nodes.find((node) => node.id === nodeMenu.nodeId)
    : undefined;
  const menuSlotId = menuNode?.kind.kind === "plugin" ? menuNode.kind.slot_id : undefined;
  const menuSlot = menuSlotId
    ? materialized.slots.find((slot) => slot.id === menuSlotId)
    : undefined;
  const editorSlot = editorSlotId
    ? materialized.slots.find((slot) => slot.id === editorSlotId)
    : undefined;
  const editorNode = editorSlotId
    ? materialized.graph!.nodes.find(
        (node) => node.kind.kind === "plugin" && node.kind.slot_id === editorSlotId,
      )
    : undefined;
  const editorInstance = editorSlot
    ? instances.find((instance) => instance.plugin_id === editorSlot.plugin_id)
    : undefined;
  const midiEditorEdge = midiLinkEditor
    ? materialized.graph!.edges.find((edge) => edge.id === midiLinkEditor.edgeId)
    : undefined;
  const midiEditorTargetNode = midiEditorEdge
    ? materialized.graph!.nodes.find((node) => node.id === midiEditorEdge.target.node_id)
    : undefined;
  const midiEditorTargetSlotId = midiEditorTargetNode?.kind.kind === "plugin"
    ? midiEditorTargetNode.kind.slot_id
    : undefined;
  const midiEditorTargetSlot = midiEditorTargetSlotId
    ? materialized.slots.find((slot) => slot.id === midiEditorTargetSlotId)
    : undefined;
  const midiEditorTargetLabel = midiEditorTargetNode
    ? nodeTitle(midiEditorTargetNode, materialized, racks)[0]
    : undefined;

  const updateSelectedLabel = useCallback(
    (patch: Partial<RackGraphLabel>) => {
      if (!selectedLabel) return;
      updateGraph((graph) => ({
        ...graph,
        labels: (graph.labels ?? []).map((label) =>
          label.id === selectedLabel.id ? { ...label, ...patch } : label,
        ),
      }));
    },
    [selectedLabel, updateGraph],
  );

  return (
    <div className="rack-graph-editor">
      {selectedLabel ? (
        <div className="rack-graph-label-tools">
          <label>
            Label
            <input
              value={selectedLabel.text}
              maxLength={512}
              onChange={(event) => updateSelectedLabel({ text: event.target.value })}
            />
          </label>
          <label>
            Tone
            <select
              value={selectedLabel.tone}
              onChange={(event) =>
                updateSelectedLabel({ tone: event.target.value as RackGraphLabelTone })
              }
            >
              {labelTones.map((tone) => (
                <option key={tone} value={tone}>
                  {tone}
                </option>
              ))}
            </select>
          </label>
        </div>
      ) : null}
      <div
        ref={canvasRef}
        className="rack-graph-canvas"
        onPointerDown={beginPaneGesture}
        onPointerMove={movePaneGesture}
        onPointerUp={finishPaneGesture}
        onPointerCancel={finishPaneGesture}
      >
        <ReactFlow<CanvasNode, RackCanvasEdge>
          nodes={interactiveNodes}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodesChange={handleNodesChange}
          onNodeDragStop={(_event, node) => persistNodePosition(node.id, node.position)}
          onInit={(instance) => {
            flowRef.current = instance;
          }}
          onConnect={connect}
          onConnectEnd={explainRefusal}
          onReconnect={reconnect}
          onReconnectStart={beginReconnect}
          onReconnectEnd={(event, edge, _handleType, state) => {
            if (reconnected.current) return;
            if (state.toHandle) {
              explainRefusal(event, state);
              return;
            }
            removeEdges([edge]);
          }}
          connectionRadius={coarsePointer ? 36 : 20}
          reconnectRadius={coarsePointer ? 24 : 10}
          isValidConnection={isValidConnection}
          onEdgesDelete={removeEdges}
          onViewportChange={setViewport}
          onNodeClick={(_event, node) => setSelectedId(node.id)}
          onNodeDoubleClick={(event, node) => {
            const graphNode = materialized.graph!.nodes.find(
              (candidate) => candidate.id === node.id,
            );
            if (graphNode?.kind.kind === "plugin") {
              setSelectedId(node.id);
              setNodeMenu(null);
              setPaneMenu(null);
              setEditorSlotId(graphNode.kind.slot_id);
            } else {
              openNodeMenu(node.id, event.clientX, event.clientY);
            }
          }}
          onNodeContextMenu={(event, node) => {
            event.preventDefault();
            openNodeMenu(node.id, event.clientX, event.clientY);
          }}
          onPaneClick={() => {
            if (suppressPaneClickRef.current) {
              suppressPaneClickRef.current = false;
              return;
            }
            setSelectedId(undefined);
            setNodeMenu(null);
            setPaneMenu(null);
            setMidiLinkEditor(null);
          }}
          onPaneContextMenu={(event) => {
            event.preventDefault();
            openPaneMenu(event.clientX, event.clientY);
          }}
          deleteKeyCode={["Backspace", "Delete"]}
          minZoom={0.2}
          maxZoom={2.5}
          zoomOnScroll
          panOnScroll={false}
          fitView
          fitViewOptions={{ padding: 0.18, maxZoom: 1.15 }}
          colorMode="dark"
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} gap={22} size={1.2} />
          {/* The minimap computes its view from its own size, so it is told
              the size it is drawn at (faceplate.css); sized only by CSS it
              kept its default 200 x 150 view inside a smaller box and showed
              the corner of it. The window onto the canvas is outlined. */}
          <MiniMap
            pannable
            zoomable
            style={{ width: MINIMAP_WIDTH, height: MINIMAP_HEIGHT }}
            nodeColor={graphMiniMapNodeColor}
            nodeBorderRadius={8}
            nodeStrokeWidth={0}
            bgColor="#1c1e22"
            maskColor="rgba(6, 7, 8, 0.66)"
            maskStrokeColor="rgba(255, 255, 255, 0.5)"
            maskStrokeWidth={1.5}
          />
          <Controls
            position="top-right"
            orientation="horizontal"
            showInteractive={false}
          >
            {history ? (
              <>
                <ControlButton
                  className="rack-history-key"
                  onClick={history.undo}
                  disabled={!history.canUndo}
                  title="Undo (Ctrl+Z)"
                  aria-label="Undo"
                >
                  <Undo2 aria-hidden="true" />
                </ControlButton>
                <ControlButton
                  className="rack-history-key"
                  onClick={history.redo}
                  disabled={!history.canRedo}
                  title="Redo (Ctrl+Shift+Z)"
                  aria-label="Redo"
                >
                  <Redo2 aria-hidden="true" />
                </ControlButton>
                <ControlButton
                  className={`rack-history-key${historyOpen ? " active" : ""}`}
                  onClick={() => setHistoryOpen((open) => !open)}
                  title="History"
                  aria-label="Editing history"
                  aria-expanded={historyOpen}
                  aria-controls="rack-history-list"
                >
                  <History aria-hidden="true" />
                </ControlButton>
              </>
            ) : null}
          </Controls>
        </ReactFlow>
        {history && historyOpen ? (
          <div id="rack-history-list" className="rack-history-list" role="dialog" aria-label="Editing history">
            <header>
              <span>History</span>
              <button type="button" onClick={() => setHistoryOpen(false)} aria-label="Close history">
                ×
              </button>
            </header>
            <ol>
              {history.entries.map((entry, index) => ({ entry, index })).reverse().map(({ entry, index }) => (
                <li key={index}>
                  <button
                    type="button"
                    className={`${entry.current ? "current" : ""}${index > history.entries.findIndex((one) => one.current) ? " undone" : ""}`}
                    aria-current={entry.current ? "step" : undefined}
                    onClick={() => history.jumpTo(index)}
                  >
                    {entry.label}
                  </button>
                </li>
              ))}
            </ol>
          </div>
        ) : null}
        {refusal ? (
          <p key={refusal.id} className="rack-graph-refusal" role="status">
            {refusal.reason}
          </p>
        ) : null}
        <div className="rack-graph-menu-layer">
          {paneMenu ? (
            <div
              className="rack-node-menu rack-pane-menu"
              style={menuStyle(paneMenu)}
              role="menu"
              aria-label="Add to Rack"
            >
              <header>
                <span>Add to Rack</span>
                <strong>{materialized.name}</strong>
              </header>
              <button type="button" role="menuitem" disabled={!canAddInstrument} onClick={() => {
                onAddInstrument(paneMenu.position, "instrument");
                setPaneMenu(null);
              }}>Instrument</button>
              <button type="button" role="menuitem" disabled={!canAddInstrument} onClick={() => {
                onAddInstrument(paneMenu.position, "effect");
                setPaneMenu(null);
              }}>Effect</button>
              <button type="button" role="menuitem" disabled={materialized.graph!.nodes.some(
                (node) => node.kind.kind === "audio_input" && node.kind.bus_id === "main",
              )} onClick={() => {
                addAudioInput(paneMenu.position);
                setPaneMenu(null);
              }}>Audio Input</button>
              <button type="button" role="menuitem" onClick={() => {
                addLabel("note", paneMenu.position);
                setPaneMenu(null);
              }}>Note</button>
              <button type="button" role="menuitem" onClick={() => {
                addLabel("section", paneMenu.position);
                setPaneMenu(null);
              }}>Section</button>
              <label className="rack-pane-child-picker">
                <span>Child Rack</span>
                <select
                  aria-label="Child Rack"
                  value={activeChildRackId}
                  onChange={(event) => setChildRackId(event.target.value)}
                  disabled={childOptions.length === 0}
                >
                  {childOptions.length === 0 ? <option>No Racks available</option> : null}
                  {childOptions.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>{candidate.name}</option>
                  ))}
                </select>
              </label>
              <button type="button" role="menuitem" disabled={!activeChildRackId} onClick={() => {
                addChildRack(paneMenu.position);
                setPaneMenu(null);
              }}>Add Child Rack</button>
              <button type="button" role="menuitem" onClick={() => setPaneMenu(null)}>Close</button>
            </div>
          ) : null}
          {nodeMenu && menuNode ? (
            <div
              className="rack-node-menu"
              style={menuStyle(nodeMenu.anchor)}
              role="menu"
              aria-label="Node actions"
            >
              <header>
                <span>{menuNode.kind.kind === "plugin" ? "Plugin" : "Node"}</span>
                <strong>{menuSlot?.name ?? nodeTitle(menuNode, materialized, racks)[0]}</strong>
              </header>
              {menuSlot ? (
                <>
                  <button type="button" role="menuitem" onClick={() => {
                    setNodeMenu(null);
                    setEditorSlotId(menuSlot.id);
                  }}>Edit plugin</button>
                </>
              ) : null}
              <button type="button" role="menuitem" onClick={() => {
                setNodeMenu(null);
                removeSelected();
              }} disabled={menuNode.kind.kind === "midi_input" || menuNode.kind.kind.endsWith("output")}>Remove node</button>
              <button type="button" role="menuitem" onClick={() => setNodeMenu(null)}>Close</button>
            </div>
          ) : null}
          {editorSlot && editorNode && editorInstance ? (
            <RackSlotPopover
              key={`${editorSlot.id}:${editorSlot.plugin_id}`}
              slot={editorSlot}
              instance={editorInstance}
              nodePosition={editorNode.position}
              viewport={viewport}
              onChange={updateSlot}
              onClose={() => setEditorSlotId(undefined)}
              renderSurface={renderPluginSurface}
            />
          ) : null}
          {midiLinkEditor && midiEditorEdge && midiEditorTargetNode ? (
            <RackMidiLinkEditor
              key={midiEditorEdge.id}
              edge={midiEditorEdge}
              fallback={midiEditorTargetSlot
                ? midiTransformFromSlot(midiEditorTargetSlot)
                : undefined}
              targetLabel={midiEditorTargetLabel}
              style={menuStyle(midiLinkEditor.anchor)}
              onClose={() => setMidiLinkEditor(null)}
              onApply={(midi_transform) => {
                updateGraph((graph) => ({
                  ...graph,
                  edges: graph.edges.map((candidate) =>
                    candidate.id === midiEditorEdge.id
                      ? { ...candidate, midi_transform }
                      : candidate,
                  ),
                }));
                setMidiLinkEditor(null);
              }}
            />
          ) : null}
        </div>
      </div>
      {problems.length > 0 ? (
        <div
          className={`rack-graph-problems${errorCount > 0 ? " has-errors" : ""}`}
          role="status"
          aria-live="polite"
        >
          <p className="rack-graph-problems-summary">
            {errorCount > 0
              ? `${errorCount} ${errorCount === 1 ? "error" : "errors"} — this Rack cannot be saved until ${errorCount === 1 ? "it is" : "they are"} resolved.`
              : `${problems.length} ${problems.length === 1 ? "warning" : "warnings"} — this Rack can be saved.`}
          </p>
          <ul>
            {problems.map((problem, index) => (
              <li key={`${problem.nodeId}:${index}`} className={problem.severity}>
                <span className="rack-graph-problem-severity">
                  {problem.severity === "error" ? "Error" : "Warning"}
                </span>
                <span>
                  <strong>
                    {mappedNodes.find((node) => node.id === problem.nodeId)?.data.title ?? problem.nodeId}
                  </strong>
                  {" — "}
                  {problem.message}
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      <p className="rack-graph-hint">
        Mouse wheel zooms · drag empty space to pan · drag ports to connect; an audio output
        feeds one place, so a new cable from it moves the old one · Delete removes a selected
        connection · double-click or hold a MIDI cable's center control to edit routing.
      </p>
    </div>
  );
}
